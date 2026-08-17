mod audio;
mod playable_catalogue;

use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    env, fs,
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use downwards_ai::{
    ComplexityBand, DifficultyConfig, InconclusiveReason, ReachedTarget, Replay, ReplayFrame,
    SearchStats, SearchTarget, SolveOutcome, SolverConfig, TargetSolveOutcome, analyze_solution,
    digest_events, solve, solve_target,
};
use downwards_catalogue::CatalogueBand;
use downwards_content::{
    AuthoredDoorRequirement, CalibratedGeneratorPlaytestLevel, CalibrationLevel,
    DEMO_DUNGEON_BOOT_PICKUP, DEMO_DUNGEON_CROWN_PICKUP, DEMO_DUNGEON_GLOVE_PICKUP,
    DEMO_DUNGEON_GOAL_EXIT, DEMO_DUNGEON_TOTAL_COINS, DemoDungeonInventory, DemoDungeonRoom,
    DemoDungeonRouteSpec, DemoDungeonRouteTarget, DungeonV2Inventory, DungeonV2Requirement,
    HARD_NO_DASH_ABILITIES, HARD_NO_DASH_TARGET, MEDIUM_NO_DASH_ABILITIES, MEDIUM_NO_DASH_TARGET,
    TraversalMethod, calibrated_generator_playtest, calibration_gallery, demo_dungeon_definition,
    demo_dungeon_door_requirement, demo_dungeon_room, demo_dungeon_route_specs,
    demo_dungeon_witness_actions, dungeon_v2_definition, dungeon_v2_door_requirement,
    dungeon_v2_exit_gate_bounds, dungeon_v2_room, dungeon_v2_total_coins, first_steps_room,
    hard_no_dash_scenario,
    hard_no_dash_witness_actions, medium_no_dash_scenario, medium_no_dash_witness_actions,
};
use downwards_core::{
    AbilitySet, Action, BoundarySide, DeathReason, HazardDirection, JUMP_BUFFER_TICKS, JumpKind,
    MovementTuning, PLAYER_MOVEMENT_POLICY_VERSION, Rect as CoreRect, Room, SUBPIXELS_PER_PIXEL,
    Simulation, SimulationEvent, StepReport, TICKS_PER_SECOND, Tile, WallSide,
};
use downwards_gen::{
    AbilityTier, CALIBRATED_WALL_JUMP_GENERATION_VERSION, COMPOSITIONAL_GENERATION_VERSION,
    DUNGEON_PALETTE_GENERATION_VERSION, experimental::GenerationStrategy, generate_compositional,
    generate_uncurated,
};
use macroquad::prelude::*;
use serde::{Deserialize, Serialize};

use crate::playable_catalogue::{PlayableCatalogue, PlayableEntry};

const LOGICAL_WIDTH: f32 = 320.0;
const LOGICAL_HEIGHT: f32 = 200.0;
const ROOM_TOP: i32 = 10;
const FIXED_STEP_SECONDS: f32 = 1.0 / TICKS_PER_SECOND as f32;
const MAX_FRAME_SECONDS: f32 = 0.25;
const HUMAN_JUMP_INPUT_POLICY_VERSION: u32 = 4;
// The checked-in catalogue and corpus witnesses predate GAMEPLAY_DEFAULT. They remain useful as
// historical evidence, but V/C must solve afresh until their wire formats bind an exact movement
// policy and are regenerated under that policy.
const HISTORICAL_CATALOGUE_WITNESS_MOVEMENT_POLICY_VERSION: u32 = 1;
/// A release inside this wall-clock window is one semantic low-jump gesture. Holding beyond it
/// commits the ordinary variable-height held input. This is deliberately a UI duration, not a
/// level-design unit.
const HUMAN_JUMP_TAP_WINDOW_MICROS: u64 = 100_000;
const HUMAN_HISTORY_SCHEMA: &str = "downwards-human-attempt-v2";
const DEFAULT_HUMAN_HISTORY_PATH: &str = "playtest-history/human-attempts-v1.jsonl";
const DUNGEON_SAVE_SCHEMA: &str = "downwards-demo-dungeon-save-v1";
const DEFAULT_DUNGEON_SAVE_PATH: &str = "playtest-history/demo-dungeon-save-v1.json";
const DEFAULT_DUNGEON_V2_SAVE_PATH: &str = "playtest-history/dungeon-v2-save-v1.json";
// The ordinary level browser starts at the first offline-curated baseline route. Raw seeds remain
// available only through the explicit developer command-line mode.
const DEFAULT_SEED: u64 = 0;
const DEATH_FEEDBACK_TICKS: u8 = 30;
const SKID_FEEDBACK_TICKS: u8 = 6;
const WALL_JUMP_FEEDBACK_TICKS: u8 = 8;
const LANDING_FEEDBACK_TICKS: u8 = 5;
const LEVEL_MENU_VISIBLE_ROWS: usize = 9;
const GALLERY_MENU_VISIBLE_ROWS: usize = 10;
const LEVEL_IDENTIFIER_VERSION: u32 = 2;

const USAGE: &str = "Downwards

Usage: downwards
       downwards [--seed <u64>] [--tier <1|2|3|4>] [--development] [--generated]
       downwards --corpus <manifest> [--tier <1|2|3|4>]
       downwards --challenge [hard|tutorial]
       downwards --gallery
       downwards --calibrated [seed]
       downwards --dungeon [--dungeon-save <path>]
       downwards --dungeon-new [--dungeon-save <path>]
       downwards --dungeon-v2 | --dungeon-v2-new [--dungeon-save <path>]
       downwards --dungeon-floor <number|id>

  (no options)   open the Downwards title screen (dungeon v2, the full game)
  --seed N       explicit developer mode: uncurated v6 seed
  --tier T       1 baseline, 2 wall jump, 3 dash, 4 wall jump + dash
  --development  start in the fixed First Steps mechanics room
  --challenge [hard|tutorial]
                 start an authored wall-jump challenge (default: hard; dash locked)
  --gallery      open the authored no-Dash calibration gallery
  --calibrated [seed]
                 play the generated WallJump-only calibration batch
  --dungeon      resume the 101-floor dungeon (or begin it if no save exists)
  --dungeon-new  archive the current save and begin a fresh dungeon run
  --dungeon-v2   play the redesigned dungeon (resumes its own save)
  --dungeon-v2-new  archive the v2 save and begin a fresh v2 run
  --dungeon-floor FLOOR
                 non-persistent lab play of one authored floor (1..101 or room id)
  --dungeon-save PATH
                 use PATH for durable dungeon progress
  --generated    open the offline-curated v6 route catalogue (the level lab)
  --corpus PATH  play the strict native-keyed corpus playtest manifest
  --history PATH append completed human attempts and input timings to PATH
  --no-audio     disable the soundtrack engine entirely
  --allow-provisional-corpus
                 explicit development opt-in for a provisional corpus export
  -h, --help     show this help

In-game lab controls:
  M              open the named level menu
  V              run the AI from the current room to its selected target
  C              run the AI from the current room to its selected coin
  F2             tune movement (applies game-wide at the next attempt)
  Jump           Space/Z/Up; tap or release early for low, hold for high
  Wall jump      touch a wall, hold toward it, then press jump
  Dash           hold a direction and press X or Shift";

const LETTERBOX: Color = Color::new(0.015, 0.02, 0.035, 1.0);
const ROOM_BACKGROUND: Color = Color::new(0.035, 0.055, 0.085, 1.0);
const SOLID: Color = Color::new(0.14, 0.18, 0.25, 1.0);
const SOLID_EXPOSED_TOP: Color = Color::new(0.38, 0.52, 0.64, 1.0);
const SOLID_EXPOSED_SIDE: Color = Color::new(0.08, 0.12, 0.19, 1.0);
const ONE_WAY: Color = Color::new(0.44, 0.57, 0.69, 1.0);
const HAZARD: Color = Color::new(0.95, 0.25, 0.34, 1.0);
const HAZARD_DARK: Color = Color::new(0.27, 0.08, 0.13, 1.0);
const SPIKE_BASE: Color = Color::new(0.04, 0.13, 0.24, 1.0);
const EXIT: Color = Color::new(0.25, 0.9, 0.7, 1.0);
const EXIT_DARK: Color = Color::new(0.06, 0.24, 0.23, 1.0);
const DOOR: Color = Color::new(0.36, 0.78, 1.0, 1.0);
const SOURCE_DOOR: Color = Color::new(0.98, 0.8, 0.28, 1.0);
const TARGET_DOOR: Color = Color::new(0.25, 0.9, 0.7, 1.0);
const PICKUP: Color = Color::new(0.98, 0.8, 0.28, 1.0);
const PLAYER: Color = Color::new(0.96, 0.97, 0.9, 1.0);
const PLAYER_ACCENT: Color = Color::new(0.35, 0.82, 0.95, 1.0);
const WALL_SLIDE_CUE: Color = Color::new(1.0, 0.78, 0.2, 1.0);
const MOVEMENT_DUST: Color = Color::new(0.46, 0.58, 0.7, 0.82);
const MOVEMENT_SPARK: Color = Color::new(0.35, 0.9, 1.0, 0.95);
const UI_TEXT: Color = Color::new(0.75, 0.82, 0.9, 1.0);
const UI_DIM: Color = Color::new(0.48, 0.56, 0.67, 1.0);
const HUD_PANEL: Color = Color::new(0.015, 0.025, 0.045, 0.88);
const DEBUG_PANEL: Color = Color::new(0.012, 0.02, 0.035, 0.94);
const DEBUG_COLLISION: Color = Color::new(0.25, 0.68, 1.0, 0.9);
const WIN_PANEL: Color = Color::new(0.025, 0.08, 0.1, 0.96);
const DEATH_PANEL: Color = Color::new(0.16, 0.025, 0.04, 0.96);
const MENU_BACKGROUND: Color = Color::new(0.022, 0.035, 0.06, 1.0);
const MENU_SELECTED: Color = Color::new(0.08, 0.19, 0.25, 1.0);

const PLAYER_SPRITE_SHEET_COLUMNS: u8 = 4;
const PLAYER_SPRITE_SHEET_ROWS: u8 = 3;
const PLAYER_SPRITE_CELL_PIXELS: f32 = 24.0;
const PLAYER_SPRITE_SHEET_WIDTH: f32 = 96.0;
const PLAYER_SPRITE_SHEET_HEIGHT: f32 = 72.0;
const PLAYER_SPRITE_LOGICAL_SIZE: f32 = 16.0;
const ENVIRONMENT_SHEET_COLUMNS: u8 = 4;
const ENVIRONMENT_SHEET_ROWS: u8 = 3;
const ENVIRONMENT_CELL_PIXELS: f32 = 24.0;
const ENVIRONMENT_SHEET_WIDTH: f32 = 96.0;
const ENVIRONMENT_SHEET_HEIGHT: f32 = 72.0;
const DUNGEON_PICKUP_CELL_PIXELS: f32 = 16.0;
const DUNGEON_PICKUP_SHEET_WIDTH: f32 = 32.0;
const DUNGEON_PICKUP_SHEET_HEIGHT: f32 = 16.0;

struct VisualAssets {
    player_sprites: Texture2D,
    environment_tiles: Texture2D,
    dungeon_pickups: Texture2D,
}

impl VisualAssets {
    fn load() -> Self {
        let player_sprites = Texture2D::from_file_with_format(
            include_bytes!("../assets/player-sprites-v2.png"),
            None,
        );
        assert_eq!(player_sprites.width(), PLAYER_SPRITE_SHEET_WIDTH);
        assert_eq!(player_sprites.height(), PLAYER_SPRITE_SHEET_HEIGHT);
        player_sprites.set_filter(FilterMode::Nearest);
        let environment_tiles = Texture2D::from_file_with_format(
            include_bytes!("../assets/environment-tiles-v2.png"),
            None,
        );
        assert_eq!(environment_tiles.width(), ENVIRONMENT_SHEET_WIDTH);
        assert_eq!(environment_tiles.height(), ENVIRONMENT_SHEET_HEIGHT);
        environment_tiles.set_filter(FilterMode::Nearest);
        let dungeon_pickups = Texture2D::from_file_with_format(
            include_bytes!("../assets/dungeon-pickups-v1.png"),
            None,
        );
        assert_eq!(dungeon_pickups.width(), DUNGEON_PICKUP_SHEET_WIDTH);
        assert_eq!(dungeon_pickups.height(), DUNGEON_PICKUP_SHEET_HEIGHT);
        dungeon_pickups.set_filter(FilterMode::Nearest);
        Self {
            player_sprites,
            environment_tiles,
            dungeon_pickups,
        }
    }
}

fn window_conf() -> Conf {
    // The plain game (title screen and both dungeon modes) is titled
    // "Downwards"; explicitly requested lab modes keep the lab title.
    let lab_mode = parse_launch_options(env::args().skip(1)).is_ok_and(|options| {
        !options.title_screen
            && !matches!(
                options.selection.mode,
                RoomMode::Dungeon | RoomMode::DungeonV2
            )
    });
    Conf {
        window_title: if lab_mode {
            "Downwards — Level Lab".to_owned()
        } else {
            "Downwards".to_owned()
        },
        window_width: 960,
        window_height: 600,
        high_dpi: false,
        window_resizable: true,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let options = match parse_launch_options(env::args().skip(1)) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    if options.show_help {
        println!("{USAGE}");
        return;
    }

    let visual_assets = VisualAssets::load();
    let mut audio = audio::AudioDirector::new(options.no_audio);
    let mut at_title = options.title_screen;
    let mut fresh_dungeon = options.fresh_dungeon;
    loop {
        if at_title {
            match run_title_screen(&options).await {
                TitleAction::Continue => fresh_dungeon = false,
                TitleAction::NewGame => fresh_dungeon = true,
                TitleAction::Quit => return,
                TitleAction::ShowControls => {
                    unreachable!("the title loop handles the controls screen itself")
                }
            }
        }
        run_play_session(&options, fresh_dungeon, &visual_assets, &mut audio).await;
        at_title = true;
    }
}

/// The title menu loop. Only returns an action that leaves the title screen.
async fn run_title_screen(options: &LaunchOptions) -> TitleAction {
    let mut menu = TitleMenuState::new(dungeon_v2_save_playable(&options.dungeon_save_path));
    let mut controls_visible = false;
    loop {
        if controls_visible {
            if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Enter) {
                controls_visible = false;
            }
        } else {
            if is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::W) {
                menu.move_up();
            } else if is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::S) {
                menu.move_down();
            }
            if is_key_pressed(KeyCode::Enter) {
                match menu.activate() {
                    Some(TitleAction::ShowControls) => controls_visible = true,
                    Some(action) => return action,
                    None => {}
                }
            } else if is_key_pressed(KeyCode::Escape) {
                menu.confirm_new_game = false;
            }
        }

        clear_background(LETTERBOX);
        let viewport = PixelViewport::for_window(screen_width(), screen_height());
        viewport.fill_logical_screen(ROOM_BACKGROUND);
        let menu_viewport = viewport.translated(0, ROOM_TOP);
        if controls_visible {
            draw_controls_screen(&menu_viewport);
        } else {
            draw_title_screen(&menu_viewport, &menu);
        }
        next_frame().await;
    }
}

/// One playtest/dungeon session; returns when the player quits to the title.
async fn run_play_session(
    options: &LaunchOptions,
    fresh_dungeon: bool,
    visual_assets: &VisualAssets,
    audio: &mut audio::AudioDirector,
) {
    let dungeon_save = matches!(
        options.selection.mode,
        RoomMode::Dungeon | RoomMode::DungeonV2
    )
    .then_some((options.dungeon_save_path.as_path(), fresh_dungeon));
    let mut client = ClientState::new_with_persistence(
        options.selection,
        options.corpus_manifest.as_deref(),
        options.allow_provisional_corpus,
        Some(&options.history_path),
        dungeon_save,
    )
    .unwrap_or_else(|error| panic!("could not start downwards: {error}"));
    eprintln!("human attempt history: {}", options.history_path.display());
    if options.selection.mode == RoomMode::Dungeon {
        eprintln!("dungeon progress: {}", options.dungeon_save_path.display());
    }
    let session_clock = Instant::now();
    let mut render_frame_index = 0_u64;
    let mut accumulated_seconds = 0.0;
    let mut dash_queued = false;
    let mut jump_input = HumanJumpInput::default();
    let mut restart_queued = false;
    let mut replay_frame_queued = false;
    let mut simulation_stepped_last_frame = false;

    loop {
        if client.quit_to_title {
            return;
        }
        if is_key_pressed(KeyCode::F3) {
            audio.toggle_mute();
        }
        // Soundtrack: follow the live room, abilities, and clock. Frames
        // that never step the simulation (menus, pauses) duck the music.
        audio.observe(&client.simulation, simulation_stepped_last_frame);
        simulation_stepped_last_frame = false;
        let render_frame = render_frame_index;
        render_frame_index = render_frame_index.saturating_add(1);
        if client.dungeon_v2_victory {
            if is_key_pressed(KeyCode::Enter) {
                client.quit_to_title = true;
            }
            accumulated_seconds = 0.0;
            dash_queued = false;
            jump_input.reset();
            restart_queued = false;
            replay_frame_queued = false;
            render(&client, visual_assets);
            next_frame().await;
            continue;
        }
        if client.level_menu_visible() {
            if client.gallery_menu_visible() {
                if is_key_pressed(KeyCode::Up)
                    || is_key_pressed(KeyCode::W)
                    || is_key_pressed(KeyCode::Left)
                    || is_key_pressed(KeyCode::A)
                {
                    client.gallery_menu.move_up();
                } else if is_key_pressed(KeyCode::Down)
                    || is_key_pressed(KeyCode::S)
                    || is_key_pressed(KeyCode::Right)
                    || is_key_pressed(KeyCode::D)
                {
                    client.gallery_menu.move_down();
                }
                if is_key_pressed(KeyCode::PageUp) {
                    client.gallery_menu.page_up();
                } else if is_key_pressed(KeyCode::PageDown) {
                    client.gallery_menu.page_down();
                } else if is_key_pressed(KeyCode::Home) {
                    client.gallery_menu.move_home();
                } else if is_key_pressed(KeyCode::End) {
                    client.gallery_menu.move_end();
                }
                client.refresh_gallery_menu_preview();

                if is_key_pressed(KeyCode::V) {
                    match client.play_gallery_selection() {
                        Ok(()) => client.request_solve(),
                        Err(error) => eprintln!("could not change gallery room: {error}"),
                    }
                } else if is_key_pressed(KeyCode::Enter) {
                    if let Err(error) = client.play_gallery_selection() {
                        eprintln!("could not change gallery room: {error}");
                    }
                } else if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::M) {
                    client.close_level_menu();
                }
            } else {
                if is_key_pressed(KeyCode::Key1) {
                    client.select_menu_tier(AbilityTier::Baseline);
                } else if is_key_pressed(KeyCode::Key2) {
                    client.select_menu_tier(AbilityTier::WallJump);
                } else if is_key_pressed(KeyCode::Key3) {
                    client.select_menu_tier(AbilityTier::Dash);
                } else if is_key_pressed(KeyCode::Key4) {
                    client.select_menu_tier(AbilityTier::WallJumpAndDash);
                } else if is_key_pressed(KeyCode::Left) || is_key_pressed(KeyCode::A) {
                    client.select_previous_menu_tier();
                } else if is_key_pressed(KeyCode::Right) || is_key_pressed(KeyCode::D) {
                    client.select_next_menu_tier();
                }
                if is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::W) {
                    client.level_menu.move_up();
                } else if is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::S) {
                    client.level_menu.move_down();
                }
                if is_key_pressed(KeyCode::PageUp) {
                    client.level_menu.page_up();
                } else if is_key_pressed(KeyCode::PageDown) {
                    client.level_menu.page_down();
                } else if is_key_pressed(KeyCode::Home) {
                    client.level_menu.move_home();
                } else if is_key_pressed(KeyCode::End) {
                    client.level_menu.move_end();
                }
                client.refresh_level_menu_preview();

                if is_key_pressed(KeyCode::C) {
                    match client.play_menu_selection() {
                        Ok(()) => client.request_pickup_solve(),
                        Err(error) => eprintln!("could not change playtest room: {error}"),
                    }
                } else if is_key_pressed(KeyCode::Enter) {
                    if let Err(error) = client.play_menu_selection() {
                        eprintln!("could not change playtest room: {error}");
                    }
                } else if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::M) {
                    client.close_level_menu();
                }
            }

            accumulated_seconds = 0.0;
            dash_queued = false;
            jump_input.reset();
            restart_queued = false;
            replay_frame_queued = false;
            render(&client, visual_assets);
            next_frame().await;
            continue;
        }

        if !client.acknowledge_menu_input_release(menu_navigation_held()) {
            accumulated_seconds = 0.0;
            dash_queued = false;
            jump_input.reset();
            restart_queued = false;
            replay_frame_queued = false;
            render(&client, visual_assets);
            next_frame().await;
            continue;
        }

        if client.movement_tuning_menu_visible() {
            if is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::W) {
                client.move_movement_tuning_selection(-1);
            } else if is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::S) {
                client.move_movement_tuning_selection(1);
            }
            if is_key_pressed(KeyCode::Left) || is_key_pressed(KeyCode::A) {
                client.adjust_movement_tuning(-1);
            } else if is_key_pressed(KeyCode::Right) || is_key_pressed(KeyCode::D) {
                client.adjust_movement_tuning(1);
            }
            if is_key_pressed(KeyCode::R) {
                client.reset_movement_tuning_draft();
            }
            if is_key_pressed(KeyCode::Escape) {
                client.cancel_movement_tuning_menu();
            } else if is_key_pressed(KeyCode::F2) || is_key_pressed(KeyCode::Enter) {
                client.close_movement_tuning_menu();
            }
            accumulated_seconds = 0.0;
            dash_queued = false;
            jump_input.reset();
            restart_queued = false;
            replay_frame_queued = false;
            render(&client, visual_assets);
            next_frame().await;
            continue;
        }

        // A requested solve renders one status frame before the synchronous search begins.
        let solve_was_requested = client.solve_requested();
        let mut performed_solve = false;
        let next_selection = selection_from_hotkeys(client.selection, &client.catalogue);
        if next_selection != client.selection {
            match client.switch_to(next_selection) {
                Ok(()) => {
                    accumulated_seconds = 0.0;
                    dash_queued = false;
                    jump_input.reset();
                    restart_queued = false;
                    replay_frame_queued = false;
                }
                Err(error) => eprintln!("could not change playtest room: {error}"),
            }
        }
        if is_key_pressed(KeyCode::F1) {
            client.debug_visible = !client.debug_visible;
        }
        if is_key_pressed(KeyCode::F2) && client.open_movement_tuning_menu() {
            accumulated_seconds = 0.0;
            dash_queued = false;
            jump_input.reset();
            restart_queued = false;
            replay_frame_queued = false;
            render(&client, visual_assets);
            next_frame().await;
            continue;
        }
        if is_key_pressed(KeyCode::Tab)
            && matches!(
                client.selection.mode,
                RoomMode::Dungeon | RoomMode::DungeonV2
            )
            && !client.level_menu_visible()
        {
            client.map_visible = !client.map_visible;
        }
        if client.pause_visible {
            if client.controls_visible {
                if is_key_pressed(KeyCode::Escape) || is_key_pressed(KeyCode::Enter) {
                    client.controls_visible = false;
                }
            } else {
                if is_key_pressed(KeyCode::Up) || is_key_pressed(KeyCode::W) {
                    client.pause_menu_index = (client.pause_menu_index + PAUSE_MENU_ITEMS.len()
                        - 1)
                        % PAUSE_MENU_ITEMS.len();
                } else if is_key_pressed(KeyCode::Down) || is_key_pressed(KeyCode::S) {
                    client.pause_menu_index =
                        (client.pause_menu_index + 1) % PAUSE_MENU_ITEMS.len();
                }
                if is_key_pressed(KeyCode::Enter) {
                    match PAUSE_MENU_ITEMS[client.pause_menu_index] {
                        "MAP" => {
                            client.pause_visible = false;
                            client.map_visible = true;
                        }
                        "CONTROLS" => client.controls_visible = true,
                        "QUIT TO TITLE" => client.quit_to_title = true,
                        _ => client.pause_visible = false,
                    }
                } else if is_key_pressed(KeyCode::Escape) {
                    client.pause_visible = false;
                }
                if is_key_pressed(KeyCode::Tab) {
                    client.pause_visible = false;
                    client.map_visible = true;
                }
            }
            accumulated_seconds = 0.0;
            dash_queued = false;
            jump_input.reset();
            restart_queued = false;
            replay_frame_queued = false;
            render(&client, visual_assets);
            next_frame().await;
            continue;
        }
        if client.map_visible {
            if is_key_pressed(KeyCode::Escape) {
                client.map_visible = false;
            }
            accumulated_seconds = 0.0;
            dash_queued = false;
            jump_input.reset();
            restart_queued = false;
            replay_frame_queued = false;
            render(&client, visual_assets);
            next_frame().await;
            continue;
        }
        if is_key_pressed(KeyCode::M) {
            client.open_level_menu();
            accumulated_seconds = 0.0;
            dash_queued = false;
            jump_input.reset();
            restart_queued = false;
            replay_frame_queued = false;
        } else if is_key_pressed(KeyCode::Escape) {
            if client.cancel_replay() {
                accumulated_seconds = 0.0;
                dash_queued = false;
                jump_input.reset();
                restart_queued = false;
                replay_frame_queued = false;
            } else if client.selection.mode == RoomMode::DungeonV2 {
                client.pause_visible = true;
                client.pause_menu_index = 0;
                client.controls_visible = false;
                accumulated_seconds = 0.0;
                dash_queued = false;
                jump_input.reset();
                restart_queued = false;
                replay_frame_queued = false;
            } else {
                client.open_level_menu();
                accumulated_seconds = 0.0;
                dash_queued = false;
                jump_input.reset();
                restart_queued = false;
                replay_frame_queued = false;
            }
        } else {
            if is_key_pressed(KeyCode::Enter)
                && client.selected_route_complete()
                && (client.human_controlled() || client.replay_complete())
            {
                if let Err(error) = client.play_next_level() {
                    eprintln!("could not load next playtest room: {error}");
                } else {
                    accumulated_seconds = 0.0;
                    dash_queued = false;
                    jump_input.reset();
                    restart_queued = false;
                    replay_frame_queued = false;
                }
            }
            if is_key_pressed(KeyCode::V) {
                client.request_solve();
                accumulated_seconds = 0.0;
                dash_queued = false;
                jump_input.reset();
                restart_queued = false;
                replay_frame_queued = false;
            } else if is_key_pressed(KeyCode::C) {
                client.request_pickup_solve();
                accumulated_seconds = 0.0;
                dash_queued = false;
                jump_input.reset();
                restart_queued = false;
                replay_frame_queued = false;
            }
            if is_key_pressed(KeyCode::H) {
                client.start_last_human_replay();
                accumulated_seconds = 0.0;
                dash_queued = false;
                jump_input.reset();
                restart_queued = false;
                replay_frame_queued = false;
            }
            if is_key_pressed(KeyCode::P) && client.toggle_replay_playback() {
                accumulated_seconds = 0.0;
                replay_frame_queued = false;
            }
            if is_key_pressed(KeyCode::N) && client.replay_paused() {
                replay_frame_queued = true;
            }
        }

        if client.level_menu_visible() {
            render(&client, visual_assets);
            next_frame().await;
            continue;
        }

        if solve_was_requested && client.solve_requested() {
            client.perform_requested_solve();
            performed_solve = true;
            accumulated_seconds = 0.0;
            dash_queued = false;
            jump_input.reset();
            restart_queued = false;
            replay_frame_queued = false;
        }

        if !performed_solve {
            accumulated_seconds += get_frame_time().min(MAX_FRAME_SECONDS);
        }
        if client.human_controlled() {
            dash_queued |= dash_pressed();
            let jump_sample = sample_raw_jump_input(
                render_frame,
                u64::try_from(session_clock.elapsed().as_micros()).unwrap_or(u64::MAX),
            );
            client.observe_human_jump_frame(jump_sample);
            let immediate_wall_jump = client.simulation.human_wall_jump_available();
            jump_input.observe_frame(
                jump_sample.aggregate_pressed(),
                jump_sample.aggregate_released(),
                jump_sample.aggregate_held(),
                jump_sample.sampled_at_session_us,
                immediate_wall_jump,
            );
            restart_queued |= is_key_pressed(KeyCode::R);
        } else {
            dash_queued = false;
            jump_input.reset();
            restart_queued = false;
        }

        while accumulated_seconds >= FIXED_STEP_SECONDS {
            simulation_stepped_last_frame = true;
            if client.human_controlled() {
                let report = client.step_human(Action {
                    move_x: horizontal_input(),
                    move_y: vertical_input(),
                    // Convert wall-clock keyboard intent into a stable semantic tap or hold.
                    jump: jump_input.action_for_tick(),
                    dash: dash_held() || dash_queued,
                    restart: restart_queued,
                });
                jump_input.observe_simulation_step(&report.events);
            } else {
                client.advance_replay(replay_frame_queued);
                if client.human_controlled() {
                    // A divergence hands control back, but never applies held human input during
                    // the remainder of the same catch-up frame.
                    accumulated_seconds = 0.0;
                    dash_queued = false;
                    jump_input.reset();
                    restart_queued = false;
                    replay_frame_queued = false;
                    break;
                }
            }
            dash_queued = false;
            restart_queued = false;
            replay_frame_queued = false;
            accumulated_seconds -= FIXED_STEP_SECONDS;
        }

        render(&client, visual_assets);
        next_frame().await;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ChallengeKind {
    Hard,
    Medium,
    DungeonFloor(DemoDungeonRoom),
}

impl ChallengeKind {
    fn level_identifier(self) -> &'static str {
        match self {
            Self::Hard => HARD_NO_DASH_LEVEL_IDENTIFIER,
            Self::Medium => MEDIUM_NO_DASH_LEVEL_IDENTIFIER,
            Self::DungeonFloor(room) => room.title(),
        }
    }

    fn target(self) -> &'static str {
        match self {
            Self::Hard => HARD_NO_DASH_TARGET,
            Self::Medium => MEDIUM_NO_DASH_TARGET,
            Self::DungeonFloor(room) => dungeon_floor_route_spec(room).target.id(),
        }
    }

    fn stats_key(self) -> String {
        match self {
            Self::Hard => "challenge:hard-no-dash:v1".to_owned(),
            Self::Medium => "challenge:medium-no-dash:v1".to_owned(),
            Self::DungeonFloor(room) => format!("dungeon-playtest:{}", room.id()),
        }
    }

    fn difficulty_label(self) -> &'static str {
        match self {
            Self::Hard => "HIGH-END",
            Self::Medium => "TUTORIAL",
            Self::DungeonFloor(_) => "FLOOR LAB · NON-PERSISTENT",
        }
    }

    fn abilities(self) -> AbilitySet {
        match self {
            Self::Hard => HARD_NO_DASH_ABILITIES,
            Self::Medium => MEDIUM_NO_DASH_ABILITIES,
            Self::DungeonFloor(room) => dungeon_floor_route_spec(room).inventory.abilities(),
        }
    }

    fn scenario(self) -> Simulation {
        match self {
            Self::Hard => hard_no_dash_scenario(),
            Self::Medium => medium_no_dash_scenario(),
            Self::DungeonFloor(room) => {
                let spec = dungeon_floor_route_spec(room);
                let room = demo_dungeon_room(room, spec.inventory);
                match spec.entry_door {
                    Some(door) => {
                        Simulation::enter_via_door(room, spec.inventory.abilities(), door)
                            .unwrap_or_else(|error| {
                                panic!("{} playtest entry {door:?} is invalid: {error}", spec.id())
                            })
                    }
                    None => Simulation::with_abilities(room, spec.inventory.abilities()),
                }
            }
        }
    }

    fn witness_actions(self) -> Vec<Action> {
        match self {
            Self::Hard => hard_no_dash_witness_actions(),
            Self::Medium => medium_no_dash_witness_actions(),
            Self::DungeonFloor(room) => demo_dungeon_witness_actions(room),
        }
    }

    fn objective(self) -> ReplayObjective {
        match self {
            Self::Hard | Self::Medium => ReplayObjective::Exit(self.target().to_owned()),
            Self::DungeonFloor(room) => match dungeon_floor_route_spec(room).target {
                DemoDungeonRouteTarget::Door(id) => ReplayObjective::Door(id.to_owned()),
                DemoDungeonRouteTarget::Pickup(id) => ReplayObjective::Pickup(id.to_owned()),
                DemoDungeonRouteTarget::GoalExit => {
                    ReplayObjective::Exit(DEMO_DUNGEON_GOAL_EXIT.to_owned())
                }
            },
        }
    }
}

/// Stable 2D layout of the dungeon graph: BFS from Hollow Landing, stepping
/// one cell per door direction and extending past occupied cells so every
/// room gets a distinct coordinate.
/// Stable 2D layout of the dungeon v2 graph, mirroring `dungeon_map_layout`:
/// BFS from the spawn, one cell per door direction, extending past occupied
/// cells so every instance gets a distinct coordinate.
fn dungeon_v2_map_layout() -> &'static HashMap<String, (i32, i32)> {
    static LAYOUT: std::sync::OnceLock<HashMap<String, (i32, i32)>> = std::sync::OnceLock::new();
    LAYOUT.get_or_init(|| {
        let dungeon = dungeon_v2_definition();
        let mut positions: HashMap<String, (i32, i32)> = HashMap::new();
        let mut occupied: HashSet<(i32, i32)> = HashSet::new();
        let mut queue = VecDeque::from([dungeon.spawn.clone()]);
        positions.insert(dungeon.spawn.clone(), (0, 0));
        occupied.insert((0, 0));
        while let Some(instance_id) = queue.pop_front() {
            let origin = positions[&instance_id];
            for (door_id, (destination, _)) in &dungeon.instances[&instance_id].connections {
                if positions.contains_key(destination) {
                    continue;
                }
                let step = match door_id.as_str() {
                    "west" => (-1, 0),
                    "east" => (1, 0),
                    "ceiling" => (0, -1),
                    _ => (0, 1),
                };
                let mut cell = (origin.0 + step.0, origin.1 + step.1);
                while occupied.contains(&cell) {
                    cell = (cell.0 + step.0, cell.1 + step.1);
                }
                positions.insert(destination.clone(), cell);
                occupied.insert(cell);
                queue.push_back(destination.clone());
            }
        }
        positions
    })
}

fn dungeon_map_layout() -> &'static HashMap<DemoDungeonRoom, (i32, i32)> {
    static LAYOUT: std::sync::OnceLock<HashMap<DemoDungeonRoom, (i32, i32)>> =
        std::sync::OnceLock::new();
    LAYOUT.get_or_init(|| {
        let definition = demo_dungeon_definition();
        let room_by_key: HashMap<u16, DemoDungeonRoom> = DemoDungeonRoom::ALL
            .into_iter()
            .map(|room| (room.authored_key().0, room))
            .collect();
        let mut positions: HashMap<DemoDungeonRoom, (i32, i32)> = HashMap::new();
        let mut occupied: HashSet<(i32, i32)> = HashSet::new();
        let mut queue = VecDeque::from([DemoDungeonRoom::HollowLanding]);
        positions.insert(DemoDungeonRoom::HollowLanding, (0, 0));
        occupied.insert((0, 0));
        while let Some(room) = queue.pop_front() {
            let origin = positions[&room];
            let floor = definition
                .floor(room.authored_key())
                .expect("room has a floor definition");
            for connection in &floor.connections {
                let destination = room_by_key[&connection.destination_floor.0];
                if positions.contains_key(&destination) {
                    continue;
                }
                let step = match connection.door_id.as_str() {
                    "west" => (-1, 0),
                    "east" => (1, 0),
                    "ceiling" => (0, -1),
                    "floor" => (0, 1),
                    _ => (1, 0),
                };
                let mut cell = (origin.0 + step.0, origin.1 + step.1);
                while occupied.contains(&cell) {
                    cell = (cell.0 + step.0, cell.1 + step.1);
                }
                positions.insert(destination, cell);
                occupied.insert(cell);
                queue.push_back(destination);
            }
        }
        positions
    })
}

fn dungeon_floor_route_spec(room: DemoDungeonRoom) -> DemoDungeonRouteSpec {
    demo_dungeon_route_specs()
        .into_iter()
        .find(|spec| spec.room == room)
        .unwrap_or_else(|| panic!("dungeon route metadata is missing {room:?}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum RoomMode {
    /// An offline-curated v6 entry. `ScenarioSelection::seed` is its tier-local index.
    Generated,
    /// An explicit raw v6 seed selected from the command line or developer hotkeys.
    DeveloperGenerated,
    Development,
    /// One entry in the stable hand-authored calibration gallery. `seed` is its index.
    Gallery,
    /// One replay-certified key in the generated human-calibration batch.
    CalibratedGenerated,
    /// Persistent multi-room dungeon vertical slice. The selection seed is unused.
    Dungeon,
    /// The redesigned data-driven dungeon (dungeon v2). The selection seed is unused.
    DungeonV2,
    /// A fixed, hand-authored validation level. Its loadout is locked by content.
    Challenge(ChallengeKind),
}

impl RoomMode {
    const fn challenge(self) -> Option<ChallengeKind> {
        match self {
            Self::Challenge(kind) => Some(kind),
            Self::Generated
            | Self::DeveloperGenerated
            | Self::Development
            | Self::Gallery
            | Self::DungeonV2
            | Self::CalibratedGenerated
            | Self::Dungeon => None,
        }
    }

    const fn is_challenge(self) -> bool {
        self.challenge().is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ScenarioSelection {
    mode: RoomMode,
    seed: u64,
    tier: AbilityTier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LevelMenuState {
    selected_index: usize,
    first_visible_index: usize,
    tier: AbilityTier,
    item_count: usize,
}

impl LevelMenuState {
    fn focused_on(selection: ScenarioSelection, curated_count: usize) -> Self {
        let mut menu = Self {
            selected_index: 0,
            first_visible_index: 0,
            tier: selection.tier,
            item_count: curated_count + 1,
        };
        match selection.mode {
            RoomMode::Development => menu.selected_index = curated_count,
            RoomMode::Generated if selection.seed < curated_count as u64 => {
                menu.selected_index = selection.seed as usize;
            }
            RoomMode::Generated
            | RoomMode::DeveloperGenerated
            | RoomMode::Gallery
            | RoomMode::CalibratedGenerated
            | RoomMode::Dungeon
            | RoomMode::DungeonV2
            | RoomMode::Challenge(_) => {
                // A raw command-line seed stays loaded behind the browser. Opening the menu
                // focuses the nearest real catalogue row, never a fabricated seed row.
                menu.selected_index = 0;
            }
        }
        menu.ensure_selected_visible();
        menu
    }

    fn visible_indices(self) -> std::ops::Range<usize> {
        let end = (self.first_visible_index + LEVEL_MENU_VISIBLE_ROWS).min(self.item_count);
        self.first_visible_index..end
    }

    fn move_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
        self.ensure_selected_visible();
    }

    fn move_down(&mut self) {
        self.selected_index = (self.selected_index + 1).min(self.item_count - 1);
        self.ensure_selected_visible();
    }

    fn page_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(LEVEL_MENU_VISIBLE_ROWS);
        self.ensure_selected_visible();
    }

    fn page_down(&mut self) {
        self.selected_index =
            (self.selected_index + LEVEL_MENU_VISIBLE_ROWS).min(self.item_count - 1);
        self.ensure_selected_visible();
    }

    fn move_home(&mut self) {
        self.selected_index = 0;
        self.ensure_selected_visible();
    }

    fn move_end(&mut self) {
        self.selected_index = self.item_count - 1;
        self.ensure_selected_visible();
    }

    fn ensure_selected_visible(&mut self) {
        if self.selected_index < self.first_visible_index {
            self.first_visible_index = self.selected_index;
        } else if self.selected_index >= self.first_visible_index + LEVEL_MENU_VISIBLE_ROWS {
            self.first_visible_index = self.selected_index + 1 - LEVEL_MENU_VISIBLE_ROWS;
        }
        let last_window_start = self.item_count.saturating_sub(LEVEL_MENU_VISIBLE_ROWS);
        self.first_visible_index = self.first_visible_index.min(last_window_start);
    }

    fn select_tier(&mut self, tier: AbilityTier, curated_count: usize) {
        let development_selected = self.selected_index + 1 == self.item_count;
        self.tier = tier;
        self.item_count = curated_count + 1;
        self.selected_index = if development_selected {
            curated_count
        } else {
            self.selected_index.min(curated_count.saturating_sub(1))
        };
        self.ensure_selected_visible();
    }

    fn selected_scenario(self) -> ScenarioSelection {
        let curated_count = self.item_count - 1;
        let (mode, seed) = if self.selected_index == curated_count {
            (RoomMode::Development, 0)
        } else {
            (RoomMode::Generated, self.selected_index as u64)
        };
        ScenarioSelection {
            mode,
            seed,
            tier: self.tier,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GalleryMenuState {
    selected_index: usize,
    first_visible_index: usize,
    item_count: usize,
}

impl GalleryMenuState {
    fn focused_on(selected_index: usize, item_count: usize) -> Self {
        let mut menu = Self {
            selected_index: selected_index.min(item_count.saturating_sub(1)),
            first_visible_index: 0,
            item_count,
        };
        menu.ensure_selected_visible();
        menu
    }

    fn visible_indices(self) -> std::ops::Range<usize> {
        let end = (self.first_visible_index + GALLERY_MENU_VISIBLE_ROWS).min(self.item_count);
        self.first_visible_index..end
    }

    fn move_up(&mut self) {
        self.selected_index = self.selected_index.saturating_sub(1);
        self.ensure_selected_visible();
    }

    fn move_down(&mut self) {
        if self.item_count > 0 {
            self.selected_index = (self.selected_index + 1).min(self.item_count - 1);
        }
        self.ensure_selected_visible();
    }

    fn page_up(&mut self) {
        self.selected_index = self
            .selected_index
            .saturating_sub(GALLERY_MENU_VISIBLE_ROWS);
        self.ensure_selected_visible();
    }

    fn page_down(&mut self) {
        if self.item_count > 0 {
            self.selected_index =
                (self.selected_index + GALLERY_MENU_VISIBLE_ROWS).min(self.item_count - 1);
        }
        self.ensure_selected_visible();
    }

    fn move_home(&mut self) {
        self.selected_index = 0;
        self.ensure_selected_visible();
    }

    fn move_end(&mut self) {
        self.selected_index = self.item_count.saturating_sub(1);
        self.ensure_selected_visible();
    }

    fn ensure_selected_visible(&mut self) {
        if self.item_count == 0 {
            self.selected_index = 0;
            self.first_visible_index = 0;
            return;
        }
        self.selected_index = self.selected_index.min(self.item_count - 1);
        if self.selected_index < self.first_visible_index {
            self.first_visible_index = self.selected_index;
        } else if self.selected_index >= self.first_visible_index + GALLERY_MENU_VISIBLE_ROWS {
            self.first_visible_index = self.selected_index + 1 - GALLERY_MENU_VISIBLE_ROWS;
        }
        self.first_visible_index = self
            .first_visible_index
            .min(self.item_count.saturating_sub(GALLERY_MENU_VISIBLE_ROWS));
    }

    fn selected_scenario(self) -> Option<ScenarioSelection> {
        (self.selected_index < self.item_count).then_some(ScenarioSelection {
            mode: RoomMode::Gallery,
            seed: self.selected_index as u64,
            tier: AbilityTier::Baseline,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrowserMode {
    Closed,
    Catalogue,
    Gallery,
    /// Non-persistent floor-lab browser over every authored dungeon room.
    DungeonFloors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GeneratedProvenance {
    generation_version: u32,
    seed: u64,
    strategy: GenerationStrategy,
    intent: downwards_gen::experimental::ChallengeIntent,
    corpus_generator: Option<&'static str>,
}

impl Default for ScenarioSelection {
    fn default() -> Self {
        Self {
            mode: RoomMode::Generated,
            seed: DEFAULT_SEED,
            tier: AbilityTier::Baseline,
        }
    }
}

impl ScenarioSelection {
    fn canonicalized(mut self) -> Self {
        if let Some(kind) = self.mode.challenge() {
            self.seed = 0;
            self.tier = tier_for_abilities(kind.abilities());
        } else if self.mode == RoomMode::CalibratedGenerated {
            self.seed %= calibrated_generator_playtest().len() as u64;
            self.tier = AbilityTier::WallJump;
        } else if self.mode == RoomMode::Dungeon {
            self.seed = 0;
            self.tier = AbilityTier::WallJump;
        } else if self.mode == RoomMode::Gallery
            && let Ok(index) = usize::try_from(self.seed)
            && let Some(level) = calibration_gallery().get(index)
        {
            self.tier = tier_for_abilities(level.abilities());
        }
        self
    }

    fn select_tier(&mut self, tier: AbilityTier) {
        if self.mode.is_challenge()
            || matches!(
                self.mode,
                RoomMode::Gallery | RoomMode::CalibratedGenerated | RoomMode::Dungeon
            )
        {
            return;
        }
        self.tier = tier;
        if self.mode == RoomMode::Generated {
            self.seed = 0;
        }
    }

    fn select_available_tier(&mut self, tier: AbilityTier, catalogue: &PlayableCatalogue) {
        if self.mode.is_challenge()
            || matches!(
                self.mode,
                RoomMode::Gallery | RoomMode::CalibratedGenerated | RoomMode::Dungeon
            )
        {
            return;
        }
        self.select_tier(tier);
        if catalogue.entries(tier).is_empty() {
            self.mode = RoomMode::Development;
        }
    }

    fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            RoomMode::Generated => RoomMode::Development,
            RoomMode::DeveloperGenerated => RoomMode::Development,
            RoomMode::Development => RoomMode::Generated,
            RoomMode::Gallery => RoomMode::Gallery,
            RoomMode::CalibratedGenerated => RoomMode::CalibratedGenerated,
            RoomMode::Dungeon => RoomMode::Dungeon,
            RoomMode::DungeonV2 => RoomMode::DungeonV2,
            RoomMode::Challenge(kind) => RoomMode::Challenge(kind),
        };
    }

    fn next_catalogue_level(self, curated_count: usize) -> Self {
        let seed = match self.mode {
            RoomMode::Generated if curated_count > 0 => (self.seed + 1) % curated_count as u64,
            RoomMode::Generated
            | RoomMode::Development
            | RoomMode::DeveloperGenerated
            | RoomMode::Gallery
            | RoomMode::CalibratedGenerated
            | RoomMode::Dungeon
            | RoomMode::DungeonV2
            | RoomMode::Challenge(_) => 0,
        };
        Self {
            mode: RoomMode::Generated,
            seed,
            tier: self.tier,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LaunchOptions {
    selection: ScenarioSelection,
    show_help: bool,
    corpus_manifest: Option<String>,
    allow_provisional_corpus: bool,
    history_path: PathBuf,
    dungeon_save_path: PathBuf,
    fresh_dungeon: bool,
    title_screen: bool,
    no_audio: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TitleMenuItem {
    Continue,
    NewGame,
    Controls,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TitleAction {
    Continue,
    NewGame,
    ShowControls,
    Quit,
}

/// The title menu over the persistent dungeon v2 save. Selecting NEW GAME
/// while a save exists demands a second confirming press before archiving it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TitleMenuState {
    save_available: bool,
    selected_index: usize,
    confirm_new_game: bool,
}

impl TitleMenuState {
    const fn new(save_available: bool) -> Self {
        Self {
            save_available,
            selected_index: 0,
            confirm_new_game: false,
        }
    }

    fn items(self) -> &'static [TitleMenuItem] {
        if self.save_available {
            &[
                TitleMenuItem::Continue,
                TitleMenuItem::NewGame,
                TitleMenuItem::Controls,
                TitleMenuItem::Quit,
            ]
        } else {
            &[
                TitleMenuItem::NewGame,
                TitleMenuItem::Controls,
                TitleMenuItem::Quit,
            ]
        }
    }

    fn selected_item(self) -> TitleMenuItem {
        self.items()[self.selected_index]
    }

    fn move_up(&mut self) {
        let count = self.items().len();
        self.selected_index = (self.selected_index + count - 1) % count;
        self.confirm_new_game = false;
    }

    fn move_down(&mut self) {
        self.selected_index = (self.selected_index + 1) % self.items().len();
        self.confirm_new_game = false;
    }

    fn activate(&mut self) -> Option<TitleAction> {
        match self.selected_item() {
            TitleMenuItem::Continue => Some(TitleAction::Continue),
            TitleMenuItem::NewGame => {
                if self.save_available && !self.confirm_new_game {
                    self.confirm_new_game = true;
                    None
                } else {
                    Some(TitleAction::NewGame)
                }
            }
            TitleMenuItem::Controls => Some(TitleAction::ShowControls),
            TitleMenuItem::Quit => Some(TitleAction::Quit),
        }
    }
}

const PAUSE_MENU_ITEMS: [&str; 4] = ["RESUME", "MAP", "CONTROLS", "QUIT TO TITLE"];

/// True when the default dungeon v2 save both parses and validates, i.e.
/// CONTINUE on the title screen has something to resume.
fn dungeon_v2_save_playable(path: &Path) -> bool {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<DungeonV2SaveV1>(&bytes).ok())
        .is_some_and(|save| save.validate().is_ok())
}

fn parse_launch_options<I, S>(arguments: I) -> Result<LaunchOptions, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut options = LaunchOptions {
        selection: ScenarioSelection::default(),
        show_help: false,
        corpus_manifest: None,
        allow_provisional_corpus: false,
        history_path: PathBuf::from(DEFAULT_HUMAN_HISTORY_PATH),
        dungeon_save_path: PathBuf::from(DEFAULT_DUNGEON_SAVE_PATH),
        fresh_dungeon: false,
        title_screen: false,
        no_audio: false,
    };
    let mut arguments = arguments.into_iter().map(Into::into).peekable();
    let mut challenge_requested = false;
    let mut gallery_requested = false;
    let mut calibrated_requested = false;
    let mut dungeon_requested = false;
    let mut dungeon_floor_requested = false;
    let mut dungeon_save_explicit = false;
    let mut authored_mode_conflict = false;
    let mut tier_explicit = false;

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => options.show_help = true,
            "--challenge" => {
                if challenge_requested {
                    return Err("--challenge may be specified only once".to_owned());
                }
                challenge_requested = true;
                let kind = match arguments.peek() {
                    Some(value) if !value.starts_with('-') => {
                        parse_challenge_kind(&arguments.next().expect("peeked challenge kind"))?
                    }
                    Some(_) | None => ChallengeKind::Hard,
                };
                options.selection.mode = RoomMode::Challenge(kind);
                options.selection.seed = 0;
                options.selection.tier = AbilityTier::WallJump;
            }
            "--gallery" => {
                if gallery_requested {
                    return Err("--gallery may be specified only once".to_owned());
                }
                gallery_requested = true;
                options.selection.mode = RoomMode::Gallery;
                options.selection.seed = 0;
                options.selection.tier = AbilityTier::Baseline;
            }
            "--calibrated" => {
                if calibrated_requested {
                    return Err("--calibrated may be specified only once".to_owned());
                }
                calibrated_requested = true;
                let seed = match arguments.peek() {
                    Some(value) if !value.starts_with('-') => {
                        parse_seed(&arguments.next().expect("peeked calibrated seed"))?
                    }
                    Some(_) | None => 0,
                };
                options.selection.mode = RoomMode::CalibratedGenerated;
                options.selection.seed = seed;
                options.selection.tier = AbilityTier::WallJump;
            }
            "--dungeon" => {
                if dungeon_requested {
                    return Err("--dungeon may be specified only once".to_owned());
                }
                dungeon_requested = true;
                options.selection.mode = RoomMode::Dungeon;
                options.selection.seed = 0;
                options.selection.tier = AbilityTier::Baseline;
            }
            "--dungeon-new" => {
                if dungeon_requested {
                    return Err("--dungeon/--dungeon-new may be specified only once".to_owned());
                }
                dungeon_requested = true;
                options.fresh_dungeon = true;
                options.selection.mode = RoomMode::Dungeon;
                options.selection.seed = 0;
                options.selection.tier = AbilityTier::Baseline;
            }
            "--dungeon-v2" | "--dungeon-v2-new" => {
                if dungeon_requested {
                    return Err("dungeon modes may be specified only once".to_owned());
                }
                dungeon_requested = true;
                options.fresh_dungeon = argument == "--dungeon-v2-new";
                options.selection.mode = RoomMode::DungeonV2;
                options.selection.seed = 0;
                options.selection.tier = AbilityTier::Baseline;
            }
            "--dungeon-floor" => {
                if dungeon_floor_requested {
                    return Err("--dungeon-floor may be specified only once".to_owned());
                }
                dungeon_floor_requested = true;
                let value = arguments
                    .next()
                    .ok_or_else(|| "--dungeon-floor needs a floor number or id".to_owned())?;
                let room = parse_dungeon_floor(&value)?;
                options.selection.mode = RoomMode::Challenge(ChallengeKind::DungeonFloor(room));
                options.selection.seed = 0;
                options.selection.tier =
                    tier_for_abilities(dungeon_floor_route_spec(room).inventory.abilities());
            }
            "--dungeon-save" => {
                dungeon_save_explicit = true;
                let path = arguments
                    .next()
                    .ok_or_else(|| "--dungeon-save needs a file path".to_owned())?;
                if path.is_empty() {
                    return Err("--dungeon-save needs a nonempty file path".to_owned());
                }
                options.dungeon_save_path = PathBuf::from(path);
            }
            "--development" => {
                authored_mode_conflict = true;
                options.selection.mode = RoomMode::Development;
            }
            "--generated" => {
                authored_mode_conflict = true;
                options.selection.mode = RoomMode::Generated;
                options.selection.seed = 0;
            }
            "--corpus" => {
                authored_mode_conflict = true;
                options.corpus_manifest = Some(
                    arguments
                        .next()
                        .ok_or_else(|| "--corpus needs a manifest path".to_owned())?,
                );
            }
            "--allow-provisional-corpus" => options.allow_provisional_corpus = true,
            "--no-audio" => options.no_audio = true,
            "--history" => {
                let path = arguments
                    .next()
                    .ok_or_else(|| "--history needs a file path".to_owned())?;
                if path.is_empty() {
                    return Err("--history needs a nonempty file path".to_owned());
                }
                options.history_path = PathBuf::from(path);
            }
            "--seed" => {
                authored_mode_conflict = true;
                let value = arguments
                    .next()
                    .ok_or_else(|| "--seed needs a value".to_owned())?;
                options.selection.seed = parse_seed(&value)?;
                options.selection.mode = RoomMode::DeveloperGenerated;
            }
            "--tier" => {
                tier_explicit = true;
                let value = arguments
                    .next()
                    .ok_or_else(|| "--tier needs a value".to_owned())?;
                options.selection.tier = parse_tier(&value)?;
            }
            _ if argument.starts_with("--seed=") => {
                authored_mode_conflict = true;
                options.selection.seed = parse_seed(&argument[7..])?;
                options.selection.mode = RoomMode::DeveloperGenerated;
            }
            _ if argument.starts_with("--tier=") => {
                tier_explicit = true;
                options.selection.tier = parse_tier(&argument[7..])?;
            }
            _ if argument.starts_with("--corpus=") => {
                authored_mode_conflict = true;
                let path = &argument[9..];
                if path.is_empty() {
                    return Err("--corpus needs a manifest path".to_owned());
                }
                options.corpus_manifest = Some(path.to_owned());
            }
            _ if argument.starts_with("--history=") => {
                let path = &argument[10..];
                if path.is_empty() {
                    return Err("--history needs a nonempty file path".to_owned());
                }
                options.history_path = PathBuf::from(path);
            }
            _ if argument.starts_with("--dungeon-save=") => {
                dungeon_save_explicit = true;
                let path = &argument[15..];
                if path.is_empty() {
                    return Err("--dungeon-save needs a nonempty file path".to_owned());
                }
                options.dungeon_save_path = PathBuf::from(path);
            }
            _ if argument.starts_with("--challenge=") => {
                if challenge_requested {
                    return Err("--challenge may be specified only once".to_owned());
                }
                challenge_requested = true;
                options.selection.mode =
                    RoomMode::Challenge(parse_challenge_kind(&argument[12..])?);
                options.selection.seed = 0;
                options.selection.tier = AbilityTier::WallJump;
            }
            _ if argument.starts_with("--calibrated=") => {
                if calibrated_requested {
                    return Err("--calibrated may be specified only once".to_owned());
                }
                calibrated_requested = true;
                options.selection.mode = RoomMode::CalibratedGenerated;
                options.selection.seed = parse_seed(&argument[13..])?;
                options.selection.tier = AbilityTier::WallJump;
            }
            _ if argument.starts_with("--dungeon-floor=") => {
                if dungeon_floor_requested {
                    return Err("--dungeon-floor may be specified only once".to_owned());
                }
                dungeon_floor_requested = true;
                let room = parse_dungeon_floor(&argument[16..])?;
                options.selection.mode = RoomMode::Challenge(ChallengeKind::DungeonFloor(room));
                options.selection.seed = 0;
                options.selection.tier =
                    tier_for_abilities(dungeon_floor_route_spec(room).inventory.abilities());
            }
            _ => return Err(format!("unknown argument {argument:?}")),
        }
    }

    if usize::from(challenge_requested)
        + usize::from(gallery_requested)
        + usize::from(calibrated_requested)
        + usize::from(dungeon_requested)
        + usize::from(dungeon_floor_requested)
        > 1
    {
        return Err(
            "--challenge, --gallery, --calibrated, --dungeon, and --dungeon-floor are mutually exclusive".to_owned(),
        );
    }
    if (challenge_requested
        || gallery_requested
        || calibrated_requested
        || dungeon_requested
        || dungeon_floor_requested)
        && authored_mode_conflict
    {
        return Err(
            "--challenge/--gallery/--calibrated/--dungeon/--dungeon-floor cannot be combined with --seed, --development, --generated, or --corpus"
                .to_owned(),
        );
    }
    if (challenge_requested
        || gallery_requested
        || calibrated_requested
        || dungeon_requested
        || dungeon_floor_requested)
        && tier_explicit
    {
        return Err(
            "--challenge/--gallery/--calibrated/--dungeon/--dungeon-floor use content-locked loadouts; omit --tier"
                .to_owned(),
        );
    }
    if challenge_requested {
        options.selection.seed = 0;
        options.selection.tier = AbilityTier::WallJump;
    }
    if calibrated_requested {
        options.selection.seed %= calibrated_generator_playtest().len() as u64;
        options.selection.tier = AbilityTier::WallJump;
    }

    // With no mode-selecting flag the client boots the dungeon v2 title screen;
    // every lab and legacy mode stays reachable through its explicit flag.
    if !(challenge_requested
        || gallery_requested
        || calibrated_requested
        || dungeon_requested
        || dungeon_floor_requested
        || authored_mode_conflict
        || tier_explicit)
    {
        options.title_screen = true;
        options.selection.mode = RoomMode::DungeonV2;
        options.selection.seed = 0;
        options.selection.tier = AbilityTier::Baseline;
    }

    if options.allow_provisional_corpus && options.corpus_manifest.is_none() {
        return Err("--allow-provisional-corpus requires --corpus".to_owned());
    }
    if dungeon_save_explicit && !dungeon_requested {
        return Err("--dungeon-save requires a dungeon mode".to_owned());
    }
    if options.selection.mode == RoomMode::DungeonV2 && !dungeon_save_explicit {
        options.dungeon_save_path = PathBuf::from(DEFAULT_DUNGEON_V2_SAVE_PATH);
    }
    if options.corpus_manifest.is_some() && options.selection.mode != RoomMode::Generated {
        return Err("--corpus cannot be combined with --seed or --development".to_owned());
    }

    Ok(options)
}

fn parse_challenge_kind(value: &str) -> Result<ChallengeKind, String> {
    match value.to_ascii_lowercase().replace('_', "-").as_str() {
        "hard" | "high-end" => Ok(ChallengeKind::Hard),
        "tutorial" | "intro" | "medium" | "mid" => Ok(ChallengeKind::Medium),
        _ => Err(format!(
            "invalid challenge {value:?}; expected hard or tutorial (medium remains an alias)"
        )),
    }
}

fn parse_dungeon_floor(value: &str) -> Result<DemoDungeonRoom, String> {
    if let Ok(number) = value.parse::<usize>() {
        return number
            .checked_sub(1)
            .and_then(|index| DemoDungeonRoom::ALL.get(index).copied())
            .ok_or_else(|| {
                format!(
                    "invalid dungeon floor {value:?}; expected a number from 1 to {}",
                    DemoDungeonRoom::ALL.len()
                )
            });
    }
    let slug = value.trim().to_ascii_lowercase().replace(['_', ' '], "-");
    let short_slug = slug.strip_prefix("demo-dungeon.").unwrap_or(&slug);
    DemoDungeonRoom::ALL
        .into_iter()
        .find(|room| {
            room.id() == slug
                || room.id().strip_prefix("demo-dungeon.") == Some(short_slug)
                || room.title().to_ascii_lowercase().replace(' ', "-") == short_slug
        })
        .ok_or_else(|| {
            format!(
                "invalid dungeon floor {value:?}; use 1..={} or an id such as gale-chasm",
                DemoDungeonRoom::ALL.len()
            )
        })
}

fn parse_seed(value: &str) -> Result<u64, String> {
    let compact = value.replace('_', "");
    let parsed = if let Some(hex) = compact
        .strip_prefix("0x")
        .or_else(|| compact.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16)
    } else {
        compact.parse()
    };
    parsed.map_err(|_| format!("invalid seed {value:?}; expected an unsigned integer"))
}

fn parse_tier(value: &str) -> Result<AbilityTier, String> {
    match value.to_ascii_lowercase().replace('_', "-").as_str() {
        "1" | "baseline" | "base" => Ok(AbilityTier::Baseline),
        "2" | "wall" | "wall-jump" | "walljump" => Ok(AbilityTier::WallJump),
        "3" | "dash" => Ok(AbilityTier::Dash),
        "4" | "all" | "wall-jump-and-dash" | "wall+dash" => Ok(AbilityTier::WallJumpAndDash),
        _ => Err(format!(
            "invalid tier {value:?}; expected 1, 2, 3, 4, baseline, wall-jump, dash, or all"
        )),
    }
}

fn selection_from_hotkeys(
    mut selection: ScenarioSelection,
    catalogue: &PlayableCatalogue,
) -> ScenarioSelection {
    if selection.mode.is_challenge() || selection.mode == RoomMode::Dungeon {
        return selection;
    }
    if selection.mode == RoomMode::CalibratedGenerated {
        let count = calibrated_generator_playtest().len();
        let next_index = if is_key_pressed(KeyCode::LeftBracket) {
            gallery_adjacent_index(selection.seed, count, false)
        } else if is_key_pressed(KeyCode::RightBracket) {
            gallery_adjacent_index(selection.seed, count, true)
        } else {
            None
        };
        if let Some(index) = next_index {
            selection.seed = index;
        }
        return selection.canonicalized();
    }
    if selection.mode == RoomMode::Gallery {
        let count = calibration_gallery().len();
        let next_index = if is_key_pressed(KeyCode::LeftBracket) {
            gallery_adjacent_index(selection.seed, count, false)
        } else if is_key_pressed(KeyCode::RightBracket) {
            gallery_adjacent_index(selection.seed, count, true)
        } else {
            None
        };
        if let Some(index) = next_index {
            selection.seed = index;
        }
        return selection.canonicalized();
    }
    if is_key_pressed(KeyCode::Tab) {
        selection.toggle_mode();
    }
    if is_key_pressed(KeyCode::LeftBracket) {
        match selection.mode {
            RoomMode::Generated => {
                let count = catalogue.entries(selection.tier).len() as u64;
                if count == 0 {
                    selection.mode = RoomMode::Development;
                    selection.seed = 0;
                } else {
                    selection.seed = (selection.seed + count - 1) % count;
                }
            }
            RoomMode::DeveloperGenerated => selection.seed = selection.seed.wrapping_sub(1),
            RoomMode::Development => {
                let count = catalogue.entries(selection.tier).len() as u64;
                if count > 0 {
                    selection.mode = RoomMode::Generated;
                    selection.seed = count - 1;
                }
            }
            RoomMode::Gallery => unreachable!("gallery hotkeys return above"),
            RoomMode::CalibratedGenerated => {
                unreachable!("calibrated hotkeys return above")
            }
            RoomMode::Dungeon | RoomMode::DungeonV2 => {
                unreachable!("dungeon hotkeys return above")
            }
            RoomMode::Challenge(_) => unreachable!("challenge hotkeys return above"),
        }
    }
    if is_key_pressed(KeyCode::RightBracket) {
        match selection.mode {
            RoomMode::Generated => {
                let count = catalogue.entries(selection.tier).len() as u64;
                if count == 0 {
                    selection.mode = RoomMode::Development;
                    selection.seed = 0;
                } else {
                    selection.seed = (selection.seed + 1) % count;
                }
            }
            RoomMode::DeveloperGenerated => selection.seed = selection.seed.wrapping_add(1),
            RoomMode::Development => {
                if !catalogue.entries(selection.tier).is_empty() {
                    selection.mode = RoomMode::Generated;
                    selection.seed = 0;
                }
            }
            RoomMode::Gallery => unreachable!("gallery hotkeys return above"),
            RoomMode::CalibratedGenerated => {
                unreachable!("calibrated hotkeys return above")
            }
            RoomMode::Dungeon | RoomMode::DungeonV2 => {
                unreachable!("dungeon hotkeys return above")
            }
            RoomMode::Challenge(_) => unreachable!("challenge hotkeys return above"),
        }
    }
    if is_key_pressed(KeyCode::Key1) {
        selection.select_available_tier(AbilityTier::Baseline, catalogue);
    } else if is_key_pressed(KeyCode::Key2) {
        selection.select_available_tier(AbilityTier::WallJump, catalogue);
    } else if is_key_pressed(KeyCode::Key3) {
        selection.select_available_tier(AbilityTier::Dash, catalogue);
    } else if is_key_pressed(KeyCode::Key4) {
        selection.select_available_tier(AbilityTier::WallJumpAndDash, catalogue);
    }
    selection
}

fn gallery_adjacent_index(current: u64, count: usize, forward: bool) -> Option<u64> {
    if count == 0 {
        return None;
    }
    let current = usize::try_from(current)
        .ok()
        .filter(|&index| index < count)
        .unwrap_or(0);
    let adjacent = if forward {
        if current + 1 == count { 0 } else { current + 1 }
    } else if current == 0 {
        count - 1
    } else {
        current - 1
    };
    Some(adjacent as u64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackPhase {
    Playing,
    Paused,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlaybackTransport {
    phase: PlaybackPhase,
    next_frame: usize,
    total_frames: usize,
}

impl PlaybackTransport {
    fn new(total_frames: usize) -> Self {
        Self {
            phase: if total_frames == 0 {
                PlaybackPhase::Complete
            } else {
                PlaybackPhase::Playing
            },
            next_frame: 0,
            total_frames,
        }
    }

    fn toggle_play_pause(&mut self) -> bool {
        match self.phase {
            PlaybackPhase::Playing => self.phase = PlaybackPhase::Paused,
            PlaybackPhase::Paused => self.phase = PlaybackPhase::Playing,
            PlaybackPhase::Complete if self.total_frames > 0 => {
                self.phase = PlaybackPhase::Playing;
                self.next_frame = 0;
            }
            PlaybackPhase::Complete => return false,
        }
        true
    }

    const fn should_advance(self, frame_step: bool) -> bool {
        matches!(self.phase, PlaybackPhase::Playing)
            || (matches!(self.phase, PlaybackPhase::Paused) && frame_step)
    }

    fn mark_advanced(&mut self) {
        debug_assert!(self.next_frame < self.total_frames);
        self.next_frame += 1;
        if self.next_frame == self.total_frames {
            self.phase = PlaybackPhase::Complete;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum AttemptOutcome {
    Died(DeathReason),
    Reset,
    Exit(String),
    WrongDoor(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum HumanJumpKey {
    Space,
    Z,
    Up,
}

impl HumanJumpKey {
    const fn index(self) -> usize {
        match self {
            Self::Space => 0,
            Self::Z => 1,
            Self::Up => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RawJumpKeySample {
    key: HumanJumpKey,
    pressed: bool,
    released: bool,
    held: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RawJumpInputSample {
    render_frame: u64,
    sampled_at_session_us: u64,
    keys: [RawJumpKeySample; 3],
}

impl RawJumpInputSample {
    fn aggregate_pressed(self) -> bool {
        self.keys.iter().any(|key| key.pressed)
    }

    fn aggregate_released(self) -> bool {
        self.keys.iter().any(|key| key.released)
    }

    fn aggregate_held(self) -> bool {
        self.keys.iter().any(|key| key.held)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenRawJumpPress {
    key: HumanJumpKey,
    press_edge_observed: bool,
    pressed_at_session_us: u64,
    pressed_at_render_frame: u64,
    sampled_frames_held: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct RawJumpPressRecord {
    key: HumanJumpKey,
    press_edge_observed: bool,
    release_edge_observed: bool,
    pressed_at_session_us: u64,
    released_at_session_us: Option<u64>,
    sampled_duration_us: Option<u64>,
    pressed_at_render_frame: u64,
    released_at_render_frame: Option<u64>,
    sampled_frames_held: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct AcceptedJumpRecord {
    tick: u64,
    kind: String,
}

impl AttemptOutcome {
    fn expected_exit(&self) -> Option<&str> {
        match self {
            Self::Exit(id) | Self::WrongDoor(id) => Some(id),
            Self::Died(_) | Self::Reset => None,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Died(_) => "DEATH",
            Self::Reset => "RESET",
            Self::Exit(_) => "SUCCESS",
            Self::WrongDoor(_) => "WRONG DOOR",
        }
    }
}

#[derive(Clone)]
struct RecordedAttempt {
    initial: Simulation,
    replay: Replay,
    outcome: AttemptOutcome,
    raw_jump_presses: Vec<RawJumpPressRecord>,
    accepted_jumps: Vec<AcceptedJumpRecord>,
}

struct AttemptInProgress {
    initial: Simulation,
    frames: Vec<ReplayFrame>,
    raw_jump_presses: Vec<RawJumpPressRecord>,
    open_raw_jump_presses: [Option<OpenRawJumpPress>; 3],
    accepted_jumps: Vec<AcceptedJumpRecord>,
}

impl AttemptInProgress {
    fn new(initial: &Simulation) -> Self {
        Self {
            initial: initial.clone(),
            frames: Vec::new(),
            raw_jump_presses: Vec::new(),
            open_raw_jump_presses: [None, None, None],
            accepted_jumps: Vec::new(),
        }
    }

    fn observe_jump_frame(&mut self, sample: RawJumpInputSample) {
        for key_sample in sample.keys {
            let slot = &mut self.open_raw_jump_presses[key_sample.key.index()];
            if slot.is_none() && (key_sample.pressed || key_sample.held) {
                *slot = Some(OpenRawJumpPress {
                    key: key_sample.key,
                    press_edge_observed: key_sample.pressed,
                    pressed_at_session_us: sample.sampled_at_session_us,
                    pressed_at_render_frame: sample.render_frame,
                    sampled_frames_held: 0,
                });
            }
            if key_sample.held
                && let Some(open) = slot
            {
                open.sampled_frames_held = open.sampled_frames_held.saturating_add(1);
            }
            if (key_sample.released || !key_sample.held)
                && let Some(open) = slot.take()
            {
                self.raw_jump_presses.push(RawJumpPressRecord {
                    key: open.key,
                    press_edge_observed: open.press_edge_observed,
                    release_edge_observed: key_sample.released,
                    pressed_at_session_us: open.pressed_at_session_us,
                    released_at_session_us: Some(sample.sampled_at_session_us),
                    sampled_duration_us: Some(
                        sample
                            .sampled_at_session_us
                            .saturating_sub(open.pressed_at_session_us),
                    ),
                    pressed_at_render_frame: open.pressed_at_render_frame,
                    released_at_render_frame: Some(sample.render_frame),
                    sampled_frames_held: open.sampled_frames_held,
                });
            }
        }
    }

    fn finish_open_jump_presses(&mut self) {
        for slot in &mut self.open_raw_jump_presses {
            let Some(open) = slot.take() else {
                continue;
            };
            self.raw_jump_presses.push(RawJumpPressRecord {
                key: open.key,
                press_edge_observed: open.press_edge_observed,
                release_edge_observed: false,
                pressed_at_session_us: open.pressed_at_session_us,
                released_at_session_us: None,
                sampled_duration_us: None,
                pressed_at_render_frame: open.pressed_at_render_frame,
                released_at_render_frame: None,
                sampled_frames_held: open.sampled_frames_held,
            });
        }
    }
}

struct HumanRecorder {
    current: Option<AttemptInProgress>,
    last_completed: Option<RecordedAttempt>,
    last_successful: Option<RecordedAttempt>,
}

impl HumanRecorder {
    fn new(initial: &Simulation) -> Self {
        Self {
            current: initial
                .reached_exit()
                .is_none()
                .then(|| AttemptInProgress::new(initial)),
            last_completed: None,
            last_successful: None,
        }
    }

    fn observe_jump_frame(&mut self, sample: RawJumpInputSample) {
        if let Some(current) = &mut self.current {
            current.observe_jump_frame(sample);
        }
    }

    fn observe_step(
        &mut self,
        action: Action,
        report: &StepReport,
        simulation: &Simulation,
        expected_target_door: Option<&str>,
    ) -> Option<AttemptOutcome> {
        if let Some(current) = &mut self.current {
            current.frames.push(ReplayFrame {
                action,
                expected_digest: report.digest,
                expected_event_digest: digest_events(&report.events),
            });
            let tick = current.frames.len() as u64;
            current
                .accepted_jumps
                .extend(report.events.iter().filter_map(|event| match event {
                    SimulationEvent::Jumped(kind) => Some(AcceptedJumpRecord {
                        tick,
                        kind: jump_kind_label(*kind),
                    }),
                    _ => None,
                }));
        }
        let outcome = attempt_outcome(&report.events, expected_target_door)?;

        let mut recorded_outcome = None;
        if let Some(mut current) = self.current.take() {
            current.finish_open_jump_presses();
            let replay = Replay {
                initial_digest: current.initial.digest(),
                frames: current.frames,
            };
            let completed = RecordedAttempt {
                initial: current.initial,
                replay,
                outcome: outcome.clone(),
                raw_jump_presses: current.raw_jump_presses,
                accepted_jumps: current.accepted_jumps,
            };
            if matches!(completed.outcome, AttemptOutcome::Exit(_)) {
                self.last_successful = Some(completed.clone());
            }
            self.last_completed = Some(completed);
            recorded_outcome = Some(outcome);
        }
        self.resume_at(simulation);
        recorded_outcome
    }

    fn preferred(&self) -> Option<&RecordedAttempt> {
        self.last_successful
            .as_ref()
            .or(self.last_completed.as_ref())
    }

    fn suspend(&mut self) {
        self.current = None;
    }

    fn resume_at(&mut self, simulation: &Simulation) {
        self.current = simulation
            .reached_exit()
            .is_none()
            .then(|| AttemptInProgress::new(simulation));
    }
}

#[derive(Serialize)]
struct PersistentActionV1 {
    move_x: i8,
    move_y: i8,
    jump: bool,
    dash: bool,
    restart: bool,
}

impl From<Action> for PersistentActionV1 {
    fn from(action: Action) -> Self {
        Self {
            move_x: action.move_x,
            move_y: action.move_y,
            jump: action.jump,
            dash: action.dash,
            restart: action.restart,
        }
    }
}

#[derive(Serialize)]
struct PersistentFrameV1 {
    tick: u64,
    action: PersistentActionV1,
    state_digest: String,
    event_digest: String,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct DeliveredJumpSpanV1 {
    start_tick: u64,
    ticks: u64,
}

#[derive(Serialize)]
struct PersistentOutcomeV1 {
    kind: &'static str,
    detail: Option<String>,
}

#[derive(Serialize)]
struct PersistentAbilitiesV1 {
    wall_jump: bool,
    dash: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct PersistentMovementTuningV1 {
    top_speed_pixels_per_second: u16,
    acceleration_milliseconds: u16,
    braking_milliseconds: u16,
    wall_ascent_carry_percent: u16,
    wall_carry_percent: u16,
    wall_momentum_milliseconds: u16,
}

impl From<MovementTuning> for PersistentMovementTuningV1 {
    fn from(tuning: MovementTuning) -> Self {
        Self {
            top_speed_pixels_per_second: tuning.top_speed_pixels_per_second,
            acceleration_milliseconds: tuning.acceleration_milliseconds,
            braking_milliseconds: tuning.braking_milliseconds,
            wall_ascent_carry_percent: tuning.wall_ascent_carry_percent,
            wall_carry_percent: tuning.wall_carry_percent,
            wall_momentum_milliseconds: tuning.wall_momentum_milliseconds,
        }
    }
}

#[derive(Serialize)]
struct PersistentAttemptV2<'a> {
    schema: &'static str,
    dungeon_definition_id: Option<&'a str>,
    palette_generation_version: Option<u32>,
    session_id: &'a str,
    session_started_unix_ms: u64,
    recorded_at_unix_ms: u64,
    attempt_index: u64,
    level_id: &'a str,
    level_name: &'a str,
    room_id: &'a str,
    human_jump_input_policy_version: u32,
    player_movement_policy_version: u32,
    movement_profile: &'static str,
    movement_tuning: Option<PersistentMovementTuningV1>,
    physics_ticks_per_second: u32,
    abilities: PersistentAbilitiesV1,
    initial_state_digest: String,
    outcome: PersistentOutcomeV1,
    total_ticks: u64,
    raw_jump_presses: &'a [RawJumpPressRecord],
    delivered_jump_spans: Vec<DeliveredJumpSpanV1>,
    accepted_jumps: &'a [AcceptedJumpRecord],
    frames: Vec<PersistentFrameV1>,
}

struct PersistentHumanHistory {
    writer: BufWriter<File>,
    session_id: String,
    session_started_unix_ms: u64,
    next_attempt_index: u64,
    next_tuning_change_index: u64,
    next_dungeon_event_index: u64,
}

#[derive(Clone, Copy)]
struct DungeonEventContext<'a> {
    event: &'a str,
    room: DemoDungeonRoom,
    destination: Option<DemoDungeonRoom>,
    door: Option<&'a str>,
    item_id: Option<&'a str>,
}

impl PersistentHumanHistory {
    fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "could not create human-history directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|error| {
                format!(
                    "could not open human-history file {}: {error}",
                    path.display()
                )
            })?;
        let session_started_unix_ms = unix_time_ms();
        Ok(Self {
            writer: BufWriter::new(file),
            session_id: format!(
                "{:016x}-{:08x}",
                session_started_unix_ms,
                std::process::id()
            ),
            session_started_unix_ms,
            next_attempt_index: 1,
            next_tuning_change_index: 1,
            next_dungeon_event_index: 1,
        })
    }

    fn append_attempt(
        &mut self,
        level_id: &str,
        level_name: &str,
        attempt: &RecordedAttempt,
    ) -> Result<(), String> {
        let dungeon_definition = (level_id.starts_with("dungeon:")
            || level_id.starts_with("dungeon-playtest:"))
        .then(demo_dungeon_definition);
        let record = PersistentAttemptV2 {
            schema: HUMAN_HISTORY_SCHEMA,
            dungeon_definition_id: dungeon_definition
                .as_ref()
                .map(|definition| definition.id.as_str()),
            palette_generation_version: dungeon_definition
                .as_ref()
                .map(|_| DUNGEON_PALETTE_GENERATION_VERSION),
            session_id: &self.session_id,
            session_started_unix_ms: self.session_started_unix_ms,
            recorded_at_unix_ms: unix_time_ms(),
            attempt_index: self.next_attempt_index,
            level_id,
            level_name,
            room_id: attempt.initial.room().id(),
            human_jump_input_policy_version: HUMAN_JUMP_INPUT_POLICY_VERSION,
            player_movement_policy_version: PLAYER_MOVEMENT_POLICY_VERSION,
            movement_profile: if attempt.initial.movement_tuning()
                == MovementTuning::GAMEPLAY_DEFAULT
            {
                "gameplay-v2"
            } else {
                "custom"
            },
            movement_tuning: Some(PersistentMovementTuningV1::from(
                attempt.initial.movement_tuning(),
            )),
            physics_ticks_per_second: TICKS_PER_SECOND,
            abilities: PersistentAbilitiesV1 {
                wall_jump: attempt.initial.abilities().wall_jump,
                dash: attempt.initial.abilities().dash,
            },
            initial_state_digest: attempt.initial.digest().to_string(),
            outcome: persistent_outcome(&attempt.outcome),
            total_ticks: attempt.replay.frames.len() as u64,
            raw_jump_presses: &attempt.raw_jump_presses,
            delivered_jump_spans: delivered_jump_spans(&attempt.replay.frames),
            accepted_jumps: &attempt.accepted_jumps,
            frames: attempt
                .replay
                .frames
                .iter()
                .enumerate()
                .map(|(index, frame)| PersistentFrameV1 {
                    tick: index as u64 + 1,
                    action: frame.action.into(),
                    state_digest: frame.expected_digest.to_string(),
                    event_digest: frame.expected_event_digest.to_string(),
                })
                .collect(),
        };
        let mut line = serde_json::to_vec(&record)
            .map_err(|error| format!("could not serialize human-history attempt: {error}"))?;
        line.push(b'\n');
        self.writer
            .write_all(&line)
            .and_then(|()| self.writer.flush())
            .map_err(|error| format!("could not persist human-history attempt: {error}"))?;
        self.next_attempt_index = self.next_attempt_index.saturating_add(1);
        Ok(())
    }

    fn append_movement_tuning_change(
        &mut self,
        level_id: &str,
        level_name: &str,
        tuning: MovementTuning,
    ) -> Result<(), String> {
        #[derive(Serialize)]
        struct PersistentMovementTuningChangeV1<'a> {
            schema: &'static str,
            session_id: &'a str,
            recorded_at_unix_ms: u64,
            change_index: u64,
            level_id: &'a str,
            level_name: &'a str,
            player_movement_policy_version: u32,
            tuning: PersistentMovementTuningV1,
        }

        let record = PersistentMovementTuningChangeV1 {
            schema: "downwards-movement-tuning-v1",
            session_id: &self.session_id,
            recorded_at_unix_ms: unix_time_ms(),
            change_index: self.next_tuning_change_index,
            level_id,
            level_name,
            player_movement_policy_version: PLAYER_MOVEMENT_POLICY_VERSION,
            tuning: tuning.into(),
        };
        let mut line = serde_json::to_vec(&record)
            .map_err(|error| format!("could not serialize movement-tuning change: {error}"))?;
        line.push(b'\n');
        self.writer
            .write_all(&line)
            .and_then(|()| self.writer.flush())
            .map_err(|error| format!("could not persist movement-tuning change: {error}"))?;
        self.next_tuning_change_index = self.next_tuning_change_index.saturating_add(1);
        Ok(())
    }

    fn append_dungeon_event(
        &mut self,
        context: DungeonEventContext<'_>,
        inventory: DemoDungeonInventory,
        movement_tuning: MovementTuning,
    ) -> Result<(), String> {
        #[derive(Serialize)]
        struct PersistentDungeonEventV2<'a> {
            schema: &'static str,
            dungeon_definition_id: &'a str,
            palette_generation_version: u32,
            session_id: &'a str,
            recorded_at_unix_ms: u64,
            event_index: u64,
            event: &'a str,
            room_id: &'static str,
            destination_room_id: Option<&'static str>,
            door_id: Option<&'a str>,
            item_id: Option<&'a str>,
            coins: u8,
            climbing_gloves: bool,
            winged_boots: bool,
            crown: bool,
            player_movement_policy_version: u32,
            movement_tuning: PersistentMovementTuningV1,
        }

        let definition = demo_dungeon_definition();
        let record = PersistentDungeonEventV2 {
            schema: "downwards-dungeon-progress-v2",
            dungeon_definition_id: &definition.id,
            palette_generation_version: DUNGEON_PALETTE_GENERATION_VERSION,
            session_id: &self.session_id,
            recorded_at_unix_ms: unix_time_ms(),
            event_index: self.next_dungeon_event_index,
            event: context.event,
            room_id: context.room.id(),
            destination_room_id: context.destination.map(DemoDungeonRoom::id),
            door_id: context.door,
            item_id: context.item_id,
            coins: inventory.coin_count(),
            climbing_gloves: inventory.climbing_gloves,
            winged_boots: inventory.winged_boots,
            crown: inventory.crown,
            player_movement_policy_version: PLAYER_MOVEMENT_POLICY_VERSION,
            movement_tuning: movement_tuning.into(),
        };
        let mut line = serde_json::to_vec(&record)
            .map_err(|error| format!("could not serialize dungeon progress: {error}"))?;
        line.push(b'\n');
        self.writer
            .write_all(&line)
            .and_then(|()| self.writer.flush())
            .map_err(|error| format!("could not persist dungeon progress: {error}"))?;
        self.next_dungeon_event_index = self.next_dungeon_event_index.saturating_add(1);
        Ok(())
    }
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}

fn persistent_outcome(outcome: &AttemptOutcome) -> PersistentOutcomeV1 {
    match outcome {
        AttemptOutcome::Died(reason) => PersistentOutcomeV1 {
            kind: "death",
            detail: Some(death_reason_label(*reason).to_ascii_lowercase()),
        },
        AttemptOutcome::Reset => PersistentOutcomeV1 {
            kind: "reset",
            detail: None,
        },
        AttemptOutcome::Exit(id) => PersistentOutcomeV1 {
            kind: "success",
            detail: Some(id.clone()),
        },
        AttemptOutcome::WrongDoor(id) => PersistentOutcomeV1 {
            kind: "wrong_door",
            detail: Some(id.clone()),
        },
    }
}

fn delivered_jump_spans(frames: &[ReplayFrame]) -> Vec<DeliveredJumpSpanV1> {
    let mut spans: Vec<DeliveredJumpSpanV1> = Vec::new();
    for (index, _) in frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| frame.action.jump)
    {
        let tick = index as u64 + 1;
        if let Some(previous) = spans.last_mut()
            && previous.start_tick + previous.ticks == tick
        {
            previous.ticks = previous.ticks.saturating_add(1);
        } else {
            spans.push(DeliveredJumpSpanV1 {
                start_tick: tick,
                ticks: 1,
            });
        }
    }
    spans
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SolverDiagnostics {
    search_stats: SearchStats,
    provisional_band: ComplexityBand,
    temporal_robustness: Option<f64>,
    accepted_wall_jumps: usize,
    accepted_dashes: usize,
}

enum ReplayOrigin {
    Solver(SolverDiagnostics),
    Challenge(ChallengeKind),
    Gallery(CalibrationLevel),
    CalibratedGenerated(CalibratedGeneratorPlaytestLevel),
    CatalogueRoute {
        band: Option<CatalogueBand>,
        stored_witness: bool,
        successful_wall_jumps: usize,
        successful_dashes: usize,
        search_stats: Option<SearchStats>,
    },
    PickupSolver {
        pickup_id: String,
        stored_witness: bool,
        search_stats: Option<SearchStats>,
    },
    Human(AttemptOutcome),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ReplayObjective {
    Exit(String),
    Door(String),
    Pickup(String),
}

impl ReplayObjective {
    fn is_satisfied_by(&self, simulation: &Simulation) -> bool {
        match self {
            Self::Exit(id) | Self::Door(id) => simulation.reached_exit() == Some(id.as_str()),
            Self::Pickup(id) => simulation
                .collected_pickups()
                .any(|pickup| pickup.id() == id),
        }
    }

    fn status_label(&self) -> String {
        match self {
            Self::Exit(id) => format!("-> {id}"),
            Self::Door(id) => format!("DOOR {id}"),
            Self::Pickup(id) => format!("COIN {id}"),
        }
    }

    fn is_satisfied_by_verification(
        &self,
        verification: &downwards_ai::ReplayVerification,
    ) -> bool {
        match self {
            Self::Exit(id) | Self::Door(id) => verification.reached_exit.as_deref() == Some(id),
            Self::Pickup(id) => verification
                .collected_pickup_ids
                .iter()
                .any(|collected| collected == id),
        }
    }
}

struct ReplayPlayback {
    initial: Simulation,
    replay: Replay,
    transport: PlaybackTransport,
    expected_objective: Option<ReplayObjective>,
    origin: ReplayOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SolveRequest {
    Route,
    Pickup(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickupSolveRequestError {
    NoPickups,
    MultiplePickups(usize),
}

enum ReplayMode {
    Human,
    SolveRequested(SolveRequest),
    Playback(Box<ReplayPlayback>),
}

struct ReplayNotice {
    title: String,
    detail: String,
    is_error: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SimulationFeedback {
    death_ticks: u8,
    last_death_reason: &'static str,
    skid_ticks: u8,
    skid_direction: i8,
    wall_jump_ticks: u8,
    wall_jump_side: Option<WallSide>,
    landing_ticks: u8,
}

impl Default for SimulationFeedback {
    fn default() -> Self {
        Self {
            death_ticks: 0,
            last_death_reason: "HAZARD",
            skid_ticks: 0,
            skid_direction: 0,
            wall_jump_ticks: 0,
            wall_jump_side: None,
            landing_ticks: 0,
        }
    }
}

impl SimulationFeedback {
    fn advance_tick(&mut self) {
        self.death_ticks = self.death_ticks.saturating_sub(1);
        self.skid_ticks = self.skid_ticks.saturating_sub(1);
        self.wall_jump_ticks = self.wall_jump_ticks.saturating_sub(1);
        self.landing_ticks = self.landing_ticks.saturating_sub(1);
        if self.wall_jump_ticks == 0 {
            self.wall_jump_side = None;
        }
    }

    fn observe(&mut self, events: &[SimulationEvent]) {
        let reset = events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Reset));
        if reset {
            self.skid_ticks = 0;
            self.skid_direction = 0;
            self.wall_jump_ticks = 0;
            self.wall_jump_side = None;
            self.landing_ticks = 0;
        }
        if !reset {
            for event in events {
                match event {
                    SimulationEvent::Jumped(JumpKind::Wall { side }) => {
                        self.wall_jump_ticks = WALL_JUMP_FEEDBACK_TICKS;
                        self.wall_jump_side = Some(*side);
                        self.skid_ticks = 0;
                    }
                    SimulationEvent::Jumped(_) => {
                        self.skid_ticks = 0;
                    }
                    SimulationEvent::Landed => {
                        self.landing_ticks = LANDING_FEEDBACK_TICKS;
                    }
                    SimulationEvent::Dashed { .. }
                    | SimulationEvent::Died(_)
                    | SimulationEvent::PickupTouched { .. }
                    | SimulationEvent::PickupCollected { .. }
                    | SimulationEvent::Reset
                    | SimulationEvent::ExitReached { .. } => {}
                }
            }
        }

        // Core deliberately emits Died followed by Reset for an automatic retry. Death feedback
        // wins for the whole report; only a standalone (manual) Reset clears an old flash.
        if let Some(reason) = events.iter().find_map(|event| match event {
            SimulationEvent::Died(reason) => Some(*reason),
            _ => None,
        }) {
            self.last_death_reason = death_reason_label(reason);
            self.death_ticks = DEATH_FEEDBACK_TICKS;
        } else if events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Reset))
        {
            self.death_ticks = 0;
        }
    }

    fn observe_motion(
        &mut self,
        action: Action,
        velocity_x_before: i32,
        grounded_before: bool,
        grounded_after: bool,
    ) {
        let direction = velocity_x_before.signum() as i8;
        let braking = action.move_x == 0 || action.move_x.signum() != direction;
        if grounded_before
            && grounded_after
            && velocity_x_before.abs() >= SUBPIXELS_PER_PIXEL
            && direction != 0
            && braking
        {
            self.skid_ticks = SKID_FEEDBACK_TICKS;
            self.skid_direction = direction;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LevelStats {
    attempts: u32,
    deaths: u32,
    clears: u32,
    best_clear_ticks: Option<usize>,
    coins_collected: u32,
}

impl LevelStats {
    fn observe_human_step(
        &mut self,
        events: &[SimulationEvent],
        outcome: Option<&AttemptOutcome>,
        attempt_ticks: Option<usize>,
    ) {
        let collected_this_tick = events
            .iter()
            .filter(|event| matches!(event, SimulationEvent::PickupCollected { .. }))
            .count() as u32;
        self.coins_collected = self.coins_collected.saturating_add(collected_this_tick);

        let Some(outcome) = outcome else {
            return;
        };
        self.attempts = self.attempts.saturating_add(1);
        match outcome {
            AttemptOutcome::Died(_) => self.deaths = self.deaths.saturating_add(1),
            AttemptOutcome::Exit(_) => {
                self.clears = self.clears.saturating_add(1);
                if let Some(ticks) = attempt_ticks {
                    self.best_clear_ticks = Some(
                        self.best_clear_ticks
                            .map_or(ticks, |previous| previous.min(ticks)),
                    );
                }
            }
            AttemptOutcome::Reset | AttemptOutcome::WrongDoor(_) => {}
        }
    }
}

struct LevelMenuPreview {
    selection: ScenarioSelection,
    simulation: Option<Simulation>,
    error: Option<String>,
}

const MOVEMENT_TUNING_ROW_COUNT: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MovementTuningMenuState {
    selected_row: usize,
    original: MovementTuning,
}

impl LevelMenuPreview {
    fn load(selection: ScenarioSelection, catalogue: &PlayableCatalogue) -> Self {
        match load_scenario(selection, catalogue) {
            Ok((simulation, _)) => Self {
                selection,
                simulation: Some(simulation),
                error: None,
            },
            Err(error) => Self {
                selection,
                simulation: None,
                error: Some(error),
            },
        }
    }
}

struct ClientState {
    catalogue: PlayableCatalogue,
    selection: ScenarioSelection,
    level_menu: LevelMenuState,
    gallery_menu: GalleryMenuState,
    level_menu_preview: LevelMenuPreview,
    browser_mode: BrowserMode,
    awaiting_menu_input_release: bool,
    generated_provenance: Option<GeneratedProvenance>,
    simulation: Simulation,
    debug_visible: bool,
    feedback: SimulationFeedback,
    human_recorder: HumanRecorder,
    human_history: Option<PersistentHumanHistory>,
    movement_tuning: MovementTuning,
    movement_tuning_menu: Option<MovementTuningMenuState>,
    level_stats: HashMap<String, LevelStats>,
    replay_mode: ReplayMode,
    replay_notice: Option<ReplayNotice>,
    dungeon_run: Option<DungeonRunState>,
    dungeon_persistence: Option<DungeonPersistence>,
    dungeon_v2_run: Option<DungeonV2RunState>,
    dungeon_v2_persistence: Option<DungeonV2Persistence>,
    explored_v2_rooms: BTreeSet<String>,
    death_pause_ticks: u8,
    explored_rooms: BTreeSet<DemoDungeonRoom>,
    map_visible: bool,
    pause_visible: bool,
    pause_menu_index: usize,
    controls_visible: bool,
    dungeon_v2_victory: bool,
    quit_to_title: bool,
    notice_seen: Option<(String, String)>,
    notice_age: u32,
}

/// Ticks of zeroed human input after a death (~0.4s at 60Hz).
const DEATH_PAUSE_TICKS: u8 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DungeonRunState {
    room: DemoDungeonRoom,
    inventory: DemoDungeonInventory,
}

impl Default for DungeonRunState {
    fn default() -> Self {
        Self {
            room: DemoDungeonRoom::HollowLanding,
            inventory: DemoDungeonInventory::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DungeonV2RunState {
    room: String,
    inventory: DungeonV2Inventory,
    deaths: u32,
    play_ticks: u64,
}

impl Default for DungeonV2RunState {
    fn default() -> Self {
        Self {
            room: dungeon_v2_definition().spawn.clone(),
            inventory: DungeonV2Inventory::default(),
            deaths: 0,
            play_ticks: 0,
        }
    }
}

const DUNGEON_V2_SAVE_SCHEMA: &str = "downwards-dungeon-v2-save-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct DungeonV2SaveV1 {
    schema: String,
    room_id: String,
    entry_door: Option<String>,
    climbing_gloves: bool,
    winged_boots: bool,
    crown: bool,
    collected_coins: Vec<String>,
    explored_rooms: Vec<String>,
    #[serde(default)]
    deaths: u32,
    #[serde(default)]
    play_ticks: u64,
    saved_at_unix_ms: u64,
}

type LoadedDungeonV2Progress = (DungeonV2RunState, Option<String>, BTreeSet<String>);

impl DungeonV2SaveV1 {
    fn from_run(
        run: &DungeonV2RunState,
        entry_door: Option<&str>,
        explored: &BTreeSet<String>,
    ) -> Self {
        Self {
            schema: DUNGEON_V2_SAVE_SCHEMA.to_owned(),
            room_id: run.room.clone(),
            entry_door: entry_door.map(str::to_owned),
            climbing_gloves: run.inventory.climbing_gloves,
            winged_boots: run.inventory.winged_boots,
            crown: run.inventory.crown,
            collected_coins: run.inventory.coins.iter().cloned().collect(),
            explored_rooms: explored.iter().cloned().collect(),
            deaths: run.deaths,
            play_ticks: run.play_ticks,
            saved_at_unix_ms: unix_time_ms(),
        }
    }

    fn validate(&self) -> Result<LoadedDungeonV2Progress, String> {
        if self.schema != DUNGEON_V2_SAVE_SCHEMA {
            return Err(format!(
                "dungeon v2 save schema {:?} is not current {:?}",
                self.schema, DUNGEON_V2_SAVE_SCHEMA
            ));
        }
        let dungeon = dungeon_v2_definition();
        if !dungeon.instances.contains_key(&self.room_id) {
            return Err(format!(
                "dungeon v2 save references unknown room {:?}",
                self.room_id
            ));
        }
        let run = DungeonV2RunState {
            room: self.room_id.clone(),
            inventory: DungeonV2Inventory {
                climbing_gloves: self.climbing_gloves,
                winged_boots: self.winged_boots,
                crown: self.crown,
                coins: self.collected_coins.iter().cloned().collect(),
            },
            deaths: self.deaths,
            play_ticks: self.play_ticks,
        };
        Ok((
            run,
            self.entry_door.clone(),
            self.explored_rooms.iter().cloned().collect(),
        ))
    }
}

struct DungeonV2Persistence {
    path: PathBuf,
}

impl DungeonV2Persistence {
    fn open(path: &Path, fresh: bool) -> Result<(Self, Option<LoadedDungeonV2Progress>), String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "could not create dungeon-save directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        if fresh && path.exists() {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("dungeon-v2-save-v1.json");
            let archive = path.with_file_name(format!("{file_name}.{}.bak", unix_time_ms()));
            fs::rename(path, &archive).map_err(|error| {
                format!(
                    "could not archive dungeon save {} as {}: {error}",
                    path.display(),
                    archive.display()
                )
            })?;
            eprintln!("archived previous dungeon v2 save: {}", archive.display());
        }
        let loaded = if path.exists() {
            let bytes = fs::read(path).map_err(|error| {
                format!("could not read dungeon v2 save {}: {error}", path.display())
            })?;
            let save: DungeonV2SaveV1 = serde_json::from_slice(&bytes).map_err(|error| {
                format!(
                    "could not parse dungeon v2 save {}: {error}",
                    path.display()
                )
            })?;
            Some(save.validate()?)
        } else {
            None
        };
        Ok((
            Self {
                path: path.to_owned(),
            },
            loaded,
        ))
    }

    fn persist(
        &self,
        run: &DungeonV2RunState,
        entry_door: Option<&str>,
        explored: &BTreeSet<String>,
    ) -> Result<(), String> {
        let save = DungeonV2SaveV1::from_run(run, entry_door, explored);
        let mut bytes = serde_json::to_vec_pretty(&save)
            .map_err(|error| format!("could not serialize dungeon v2 save: {error}"))?;
        bytes.push(b'\n');
        let temporary = self.path.with_extension("tmp");
        fs::write(&temporary, &bytes)
            .map_err(|error| format!("could not write dungeon v2 save: {error}"))?;
        fs::rename(&temporary, &self.path).map_err(|error| {
            format!(
                "could not publish dungeon v2 save {}: {error}",
                self.path.display()
            )
        })?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct DungeonSaveV1 {
    schema: String,
    dungeon_id: String,
    palette_generation_version: u32,
    floor_count: u16,
    total_coins: u8,
    room_id: String,
    entry_door: Option<String>,
    climbing_gloves: bool,
    winged_boots: bool,
    crown: bool,
    collected_coins: Vec<u8>,
    #[serde(default)]
    explored_rooms: Vec<String>,
    saved_at_unix_ms: u64,
}

impl DungeonSaveV1 {
    fn from_run(
        run: DungeonRunState,
        entry_door: Option<&str>,
        explored: &BTreeSet<DemoDungeonRoom>,
    ) -> Self {
        Self {
            schema: DUNGEON_SAVE_SCHEMA.to_owned(),
            dungeon_id: demo_dungeon_definition().id,
            palette_generation_version: DUNGEON_PALETTE_GENERATION_VERSION,
            floor_count: DemoDungeonRoom::ALL.len() as u16,
            total_coins: DEMO_DUNGEON_TOTAL_COINS,
            room_id: run.room.id().to_owned(),
            entry_door: entry_door.map(str::to_owned),
            climbing_gloves: run.inventory.climbing_gloves,
            winged_boots: run.inventory.winged_boots,
            crown: run.inventory.crown,
            collected_coins: (0..DEMO_DUNGEON_TOTAL_COINS)
                .filter(|index| run.inventory.has_coin(&format!("dungeon-coin-{index:02}")))
                .collect(),
            explored_rooms: explored.iter().map(|room| room.id().to_owned()).collect(),
            saved_at_unix_ms: unix_time_ms(),
        }
    }

    fn validate(&self) -> Result<LoadedDungeonProgress, String> {
        if self.schema != DUNGEON_SAVE_SCHEMA {
            return Err(format!(
                "dungeon save schema {:?} is not current {:?}",
                self.schema, DUNGEON_SAVE_SCHEMA
            ));
        }
        let definition = demo_dungeon_definition();
        if self.dungeon_id != definition.id
            || self.palette_generation_version != DUNGEON_PALETTE_GENERATION_VERSION
            || self.floor_count != DemoDungeonRoom::ALL.len() as u16
            || self.total_coins != DEMO_DUNGEON_TOTAL_COINS
        {
            return Err("dungeon save belongs to a different authored dungeon version".to_owned());
        }
        if self
            .collected_coins
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
            || self
                .collected_coins
                .iter()
                .any(|index| *index >= DEMO_DUNGEON_TOTAL_COINS)
        {
            return Err(
                "dungeon save coin indices must be unique, sorted, and in range".to_owned(),
            );
        }
        if self.winged_boots && !self.climbing_gloves {
            return Err("dungeon save grants Dash without the earlier Wall Jump unlock".to_owned());
        }
        if self.crown
            && (!self.winged_boots
                || self.collected_coins.len()
                    < usize::from(downwards_content::DEMO_DUNGEON_CROWN_GATE_REQUIREMENT))
        {
            return Err("dungeon save grants the Crown without full progression".to_owned());
        }
        let room = DemoDungeonRoom::from_id(&self.room_id)
            .ok_or_else(|| format!("dungeon save names unknown room {:?}", self.room_id))?;
        let mut inventory = DemoDungeonInventory::default();
        inventory.climbing_gloves = self.climbing_gloves;
        inventory.winged_boots = self.winged_boots;
        inventory.crown = self.crown;
        for index in &self.collected_coins {
            let inserted = inventory.collect_coin(&format!("dungeon-coin-{index:02}"));
            debug_assert!(inserted);
        }
        let restored_room = demo_dungeon_room(room, inventory);
        match self.entry_door.as_deref() {
            Some(entry) if !restored_room.doors().iter().any(|door| door.id == entry) => {
                return Err(format!(
                    "dungeon save entry door {entry:?} does not exist in {:?}",
                    self.room_id
                ));
            }
            None if room != DemoDungeonRoom::HollowLanding => {
                return Err("only the initial dungeon room may omit its entry door".to_owned());
            }
            Some(_) | None => {}
        }
        let mut explored: BTreeSet<DemoDungeonRoom> = self
            .explored_rooms
            .iter()
            .map(|id| {
                DemoDungeonRoom::from_id(id)
                    .ok_or_else(|| format!("dungeon save explored unknown room {id:?}"))
            })
            .collect::<Result<_, _>>()?;
        explored.insert(room);
        Ok((
            DungeonRunState { room, inventory },
            self.entry_door.clone(),
            explored,
        ))
    }
}

#[derive(Debug)]
struct DungeonPersistence {
    path: PathBuf,
}

type LoadedDungeonProgress = (DungeonRunState, Option<String>, BTreeSet<DemoDungeonRoom>);

impl DungeonPersistence {
    fn open(path: &Path, fresh: bool) -> Result<(Self, Option<LoadedDungeonProgress>), String> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "could not create dungeon-save directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        if fresh && path.exists() {
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("demo-dungeon-save-v1.json");
            let mut archive = path.with_file_name(format!("{file_name}.{}.bak", unix_time_ms()));
            let mut suffix = 1_u16;
            while archive.exists() {
                archive =
                    path.with_file_name(format!("{file_name}.{}.{}.bak", unix_time_ms(), suffix));
                suffix = suffix.saturating_add(1);
            }
            fs::rename(path, &archive).map_err(|error| {
                format!(
                    "could not archive dungeon save {} as {}: {error}",
                    path.display(),
                    archive.display()
                )
            })?;
            eprintln!("archived previous dungeon save: {}", archive.display());
        }
        let loaded = if path.exists() {
            let bytes = fs::read(path).map_err(|error| {
                format!("could not read dungeon save {}: {error}", path.display())
            })?;
            let save: DungeonSaveV1 = serde_json::from_slice(&bytes).map_err(|error| {
                format!("could not parse dungeon save {}: {error}", path.display())
            })?;
            Some(save.validate()?)
        } else {
            None
        };
        Ok((
            Self {
                path: path.to_owned(),
            },
            loaded,
        ))
    }

    fn persist(
        &self,
        run: DungeonRunState,
        entry_door: Option<&str>,
        explored: &BTreeSet<DemoDungeonRoom>,
    ) -> Result<(), String> {
        let save = DungeonSaveV1::from_run(run, entry_door, explored);
        let mut bytes = serde_json::to_vec_pretty(&save)
            .map_err(|error| format!("could not serialize dungeon save: {error}"))?;
        bytes.push(b'\n');
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("demo-dungeon-save-v1.json");
        let temporary = self.path.with_file_name(format!(
            ".{file_name}.tmp-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                format!(
                    "could not create temporary dungeon save {}: {error}",
                    temporary.display()
                )
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| format!("could not write dungeon save: {error}"))?;
        fs::rename(&temporary, &self.path).map_err(|error| {
            format!(
                "could not publish dungeon save {}: {error}",
                self.path.display()
            )
        })?;
        let readback = fs::read(&self.path).map_err(|error| {
            format!(
                "could not read back dungeon save {}: {error}",
                self.path.display()
            )
        })?;
        let parsed: DungeonSaveV1 = serde_json::from_slice(&readback)
            .map_err(|error| format!("published dungeon save is not readable: {error}"))?;
        if parsed.validate()? != (run, entry_door.map(str::to_owned), explored.clone()) {
            return Err("published dungeon save readback changed the run state".to_owned());
        }
        Ok(())
    }
}

impl ClientState {
    #[cfg(test)]
    fn new(
        selection: ScenarioSelection,
        corpus_manifest: Option<&str>,
        allow_provisional_corpus: bool,
    ) -> Result<Self, String> {
        Self::new_with_history(selection, corpus_manifest, allow_provisional_corpus, None)
    }

    #[cfg(test)]
    fn new_with_history(
        selection: ScenarioSelection,
        corpus_manifest: Option<&str>,
        allow_provisional_corpus: bool,
        history_path: Option<&Path>,
    ) -> Result<Self, String> {
        Self::new_with_persistence(
            selection,
            corpus_manifest,
            allow_provisional_corpus,
            history_path,
            None,
        )
    }

    fn new_with_persistence(
        mut selection: ScenarioSelection,
        corpus_manifest: Option<&str>,
        allow_provisional_corpus: bool,
        history_path: Option<&Path>,
        dungeon_save: Option<(&Path, bool)>,
    ) -> Result<Self, String> {
        selection = selection.canonicalized();
        let catalogue = load_playable_catalogue(corpus_manifest, allow_provisional_corpus)?;
        if selection.mode == RoomMode::Generated && catalogue.entries(selection.tier).is_empty() {
            selection.mode = RoomMode::Development;
            selection.seed = 0;
        }
        let (mut simulation, generated_provenance) = load_scenario(selection, &catalogue)?;
        let mut dungeon_run = (selection.mode == RoomMode::Dungeon).then(DungeonRunState::default);
        let mut explored_rooms = BTreeSet::new();
        if dungeon_run.is_some() {
            explored_rooms.insert(DemoDungeonRoom::HollowLanding);
        }
        let dungeon_persistence = match (selection.mode, dungeon_save) {
            (RoomMode::Dungeon, Some((path, fresh))) => {
                let (persistence, loaded) = DungeonPersistence::open(path, fresh)?;
                if let Some((loaded_run, entry_door, loaded_explored)) = loaded {
                    explored_rooms = loaded_explored;
                    let room = demo_dungeon_room(loaded_run.room, loaded_run.inventory);
                    simulation = match entry_door.as_deref() {
                        Some(door) => {
                            Simulation::enter_via_door(room, loaded_run.inventory.abilities(), door)
                                .map_err(|error| {
                                    format!("could not restore dungeon entry: {error}")
                                })?
                        }
                        None => Simulation::with_abilities(room, loaded_run.inventory.abilities()),
                    };
                    dungeon_run = Some(loaded_run);
                }
                let run = dungeon_run.expect("dungeon mode initializes run state");
                persistence.persist(run, simulation.entry_door(), &explored_rooms)?;
                Some(persistence)
            }
            (RoomMode::Dungeon, None) => None,
            (RoomMode::DungeonV2, _) => None,
            (_, Some(_)) => {
                return Err("dungeon persistence requires dungeon mode".to_owned());
            }
            (_, None) => None,
        };
        let mut dungeon_v2_run =
            (selection.mode == RoomMode::DungeonV2).then(DungeonV2RunState::default);
        let mut explored_v2_rooms = BTreeSet::new();
        if dungeon_v2_run.is_some() {
            explored_v2_rooms.insert(dungeon_v2_definition().spawn.clone());
        }
        let dungeon_v2_persistence = if selection.mode == RoomMode::DungeonV2
            && let Some((path, fresh)) = dungeon_save
        {
            let (persistence, loaded) = DungeonV2Persistence::open(path, fresh)?;
            if let Some((loaded_run, entry_door, loaded_explored)) = loaded {
                explored_v2_rooms = loaded_explored;
                explored_v2_rooms.insert(loaded_run.room.clone());
                let room = dungeon_v2_room(&loaded_run.room, &loaded_run.inventory);
                simulation = match entry_door.as_deref() {
                    Some(door) => {
                        Simulation::enter_via_door(room, loaded_run.inventory.abilities(), door)
                            .map_err(|error| {
                                format!("could not restore dungeon v2 entry: {error}")
                            })?
                    }
                    None => Simulation::with_abilities(room, loaded_run.inventory.abilities()),
                };
                dungeon_v2_run = Some(loaded_run);
            }
            let run = dungeon_v2_run
                .as_ref()
                .expect("dungeon v2 mode initializes run state");
            persistence.persist(run, simulation.entry_door(), &explored_v2_rooms)?;
            Some(persistence)
        } else {
            None
        };
        let movement_tuning = MovementTuning::GAMEPLAY_DEFAULT;
        configure_live_simulation(&mut simulation, movement_tuning);
        let human_recorder = HumanRecorder::new(&simulation);
        let human_history = history_path.map(PersistentHumanHistory::open).transpose()?;
        let level_menu =
            LevelMenuState::focused_on(selection, catalogue.entries(selection.tier).len());
        let gallery_menu = GalleryMenuState::focused_on(
            usize::try_from(selection.seed).unwrap_or(0),
            calibration_gallery().len(),
        );
        let preview_selection = if selection.mode == RoomMode::Gallery {
            gallery_menu
                .selected_scenario()
                .ok_or_else(|| "calibration gallery has no playable levels".to_owned())?
        } else {
            level_menu.selected_scenario()
        };
        let level_menu_preview = LevelMenuPreview::load(preview_selection, &catalogue);
        let browser_mode = match selection.mode {
            RoomMode::Gallery => BrowserMode::Gallery,
            RoomMode::Challenge(_)
            | RoomMode::CalibratedGenerated
            | RoomMode::Dungeon
            | RoomMode::DungeonV2 => BrowserMode::Closed,
            RoomMode::Generated | RoomMode::DeveloperGenerated | RoomMode::Development => {
                BrowserMode::Catalogue
            }
        };
        Ok(Self {
            catalogue,
            selection,
            level_menu,
            gallery_menu,
            level_menu_preview,
            browser_mode,
            awaiting_menu_input_release: false,
            generated_provenance,
            simulation,
            debug_visible: false,
            feedback: SimulationFeedback::default(),
            human_recorder,
            human_history,
            movement_tuning,
            movement_tuning_menu: None,
            level_stats: HashMap::new(),
            replay_mode: ReplayMode::Human,
            replay_notice: None,
            dungeon_run,
            dungeon_persistence,
            dungeon_v2_run,
            dungeon_v2_persistence,
            explored_v2_rooms,
            death_pause_ticks: 0,
            explored_rooms,
            map_visible: false,
            pause_visible: false,
            pause_menu_index: 0,
            controls_visible: false,
            dungeon_v2_victory: false,
            quit_to_title: false,
            notice_seen: None,
            notice_age: 0,
        })
    }

    fn switch_to(&mut self, selection: ScenarioSelection) -> Result<(), String> {
        let selection = selection.canonicalized();
        let (mut simulation, generated_provenance) = load_scenario(selection, &self.catalogue)?;
        configure_live_simulation(&mut simulation, self.movement_tuning);
        self.selection = selection;
        self.generated_provenance = generated_provenance;
        self.simulation = simulation;
        self.feedback = SimulationFeedback::default();
        self.human_recorder = HumanRecorder::new(&self.simulation);
        self.replay_mode = ReplayMode::Human;
        self.replay_notice = None;
        self.movement_tuning_menu = None;
        self.dungeon_run = (selection.mode == RoomMode::Dungeon).then(DungeonRunState::default);
        self.dungeon_persistence = None;
        self.dungeon_v2_run =
            (selection.mode == RoomMode::DungeonV2).then(DungeonV2RunState::default);
        self.dungeon_v2_persistence = None;
        self.explored_v2_rooms = BTreeSet::new();
        self.explored_rooms = if selection.mode == RoomMode::Dungeon {
            BTreeSet::from([DemoDungeonRoom::HollowLanding])
        } else {
            BTreeSet::new()
        };
        self.map_visible = false;
        self.pause_visible = false;
        self.pause_menu_index = 0;
        self.controls_visible = false;
        self.dungeon_v2_victory = false;
        Ok(())
    }

    const fn level_menu_visible(&self) -> bool {
        !matches!(self.browser_mode, BrowserMode::Closed)
    }

    const fn gallery_menu_visible(&self) -> bool {
        matches!(
            self.browser_mode,
            BrowserMode::Gallery | BrowserMode::DungeonFloors
        )
    }

    fn current_dungeon_room(&self) -> Option<DemoDungeonRoom> {
        match self.selection.mode {
            RoomMode::Challenge(ChallengeKind::DungeonFloor(room)) => Some(room),
            RoomMode::Dungeon => Some(
                self.dungeon_run
                    .map_or(DemoDungeonRoom::HollowLanding, |run| run.room),
            ),
            _ => None,
        }
    }

    fn open_level_menu(&mut self) {
        if let Some(room) = self.current_dungeon_room() {
            let focused = DemoDungeonRoom::ALL
                .iter()
                .position(|candidate| *candidate == room)
                .unwrap_or(0);
            self.gallery_menu = GalleryMenuState::focused_on(focused, DemoDungeonRoom::ALL.len());
            self.browser_mode = BrowserMode::DungeonFloors;
            self.refresh_gallery_menu_preview();
            return;
        }
        if self.selection.mode == RoomMode::Gallery {
            self.gallery_menu = GalleryMenuState::focused_on(
                usize::try_from(self.selection.seed).unwrap_or(0),
                calibration_gallery().len(),
            );
            self.refresh_gallery_menu_preview();
            self.browser_mode = BrowserMode::Gallery;
            return;
        }
        // Generated calibration is launch-only like a challenge. M deliberately
        // opens the ordinary named browser; brackets cycle the generated batch.
        self.level_menu = LevelMenuState::focused_on(
            self.selection,
            self.catalogue.entries(self.selection.tier).len(),
        );
        self.refresh_level_menu_preview();
        self.browser_mode = BrowserMode::Catalogue;
    }

    fn refresh_level_menu_preview(&mut self) -> bool {
        let selection = self.level_menu.selected_scenario();
        if self.level_menu_preview.selection == selection {
            return false;
        }
        self.level_menu_preview = LevelMenuPreview::load(selection, &self.catalogue);
        true
    }

    fn selected_dungeon_floor_scenario(&self) -> Option<ScenarioSelection> {
        let room = DemoDungeonRoom::ALL.get(self.gallery_menu.selected_index)?;
        Some(
            ScenarioSelection {
                mode: RoomMode::Challenge(ChallengeKind::DungeonFloor(*room)),
                seed: 0,
                tier: AbilityTier::Baseline,
            }
            .canonicalized(),
        )
    }

    fn selected_browser_scenario(&self) -> Option<ScenarioSelection> {
        if self.browser_mode == BrowserMode::DungeonFloors {
            self.selected_dungeon_floor_scenario()
        } else {
            self.gallery_menu.selected_scenario()
        }
    }

    fn refresh_gallery_menu_preview(&mut self) -> bool {
        let Some(selection) = self.selected_browser_scenario() else {
            return false;
        };
        if self.level_menu_preview.selection == selection {
            return false;
        }
        self.level_menu_preview = LevelMenuPreview::load(selection, &self.catalogue);
        true
    }

    fn select_menu_tier(&mut self, tier: AbilityTier) {
        self.level_menu
            .select_tier(tier, self.catalogue.entries(tier).len());
    }

    fn select_previous_menu_tier(&mut self) {
        self.select_menu_tier(previous_ability_tier(self.level_menu.tier));
    }

    fn select_next_menu_tier(&mut self) {
        self.select_menu_tier(next_ability_tier(self.level_menu.tier));
    }

    fn close_level_menu(&mut self) {
        self.browser_mode = BrowserMode::Closed;
        self.awaiting_menu_input_release = true;
    }

    fn play_menu_selection(&mut self) -> Result<(), String> {
        let selection = self.level_menu.selected_scenario();
        self.switch_to(selection)?;
        self.browser_mode = BrowserMode::Closed;
        self.awaiting_menu_input_release = true;
        Ok(())
    }

    fn play_gallery_selection(&mut self) -> Result<(), String> {
        let selection = self
            .selected_browser_scenario()
            .ok_or_else(|| "this browser has no playable levels".to_owned())?;
        self.switch_to(selection)?;
        self.browser_mode = BrowserMode::Closed;
        self.awaiting_menu_input_release = true;
        Ok(())
    }

    fn play_next_level(&mut self) -> Result<(), String> {
        let selection = self
            .next_level_selection()
            .ok_or_else(|| "there is no next playable level".to_owned())?;
        self.switch_to(selection)
    }

    fn next_level_selection(&self) -> Option<ScenarioSelection> {
        if self.selection.mode == RoomMode::Dungeon {
            return None;
        }
        // Floor-lab rooms continue through their own doors, never into the
        // generated catalogue.
        if let RoomMode::Challenge(ChallengeKind::DungeonFloor(_)) = self.selection.mode {
            return None;
        }
        if self.selection.mode == RoomMode::Gallery {
            let index =
                gallery_adjacent_index(self.selection.seed, calibration_gallery().len(), true)?;
            return Some(ScenarioSelection {
                mode: RoomMode::Gallery,
                seed: index,
                tier: AbilityTier::Baseline,
            });
        }
        if self.selection.mode == RoomMode::CalibratedGenerated {
            let index = gallery_adjacent_index(
                self.selection.seed,
                calibrated_generator_playtest().len(),
                true,
            )?;
            return Some(ScenarioSelection {
                mode: RoomMode::CalibratedGenerated,
                seed: index,
                tier: AbilityTier::WallJump,
            });
        }
        let count = self.catalogue.entries(self.selection.tier).len();
        Some(self.selection.next_catalogue_level(count))
    }

    fn stats_for(&self, selection: ScenarioSelection) -> LevelStats {
        self.level_stats
            .get(&self.stats_key(selection))
            .copied()
            .unwrap_or_default()
    }

    fn level_name(&self, selection: ScenarioSelection) -> String {
        match selection.mode {
            RoomMode::Generated => self.catalogue_entry(selection).map_or_else(
                || "MISSING CURATED ROOM".to_owned(),
                |entry| self.catalogue.name(entry).to_owned(),
            ),
            RoomMode::DeveloperGenerated => format!("DEV SEED {:X}", selection.seed),
            RoomMode::Development => DEVELOPMENT_LEVEL_IDENTIFIER.to_owned(),
            RoomMode::Gallery => self.gallery_entry(selection).map_or_else(
                || "MISSING GALLERY LEVEL".to_owned(),
                |level| format!("{} {}", level.id().to_ascii_uppercase(), level.title()),
            ),
            RoomMode::CalibratedGenerated => self.calibrated_entry(selection).map_or_else(
                || "MISSING CALIBRATED LEVEL".to_owned(),
                |level| level.title(),
            ),
            RoomMode::Dungeon => self.dungeon_run.map_or_else(
                || "MISSING DUNGEON RUN".to_owned(),
                |run| format!("DUNGEON · {}", run.room.title()),
            ),
            RoomMode::DungeonV2 => self.dungeon_v2_run.as_ref().map_or_else(
                || "DUNGEON V2".to_owned(),
                |run| format!("DUNGEON V2 · {}", run.room.to_ascii_uppercase()),
            ),
            RoomMode::Challenge(kind) => kind.level_identifier().to_owned(),
        }
    }

    fn stats_key(&self, selection: ScenarioSelection) -> String {
        match selection.mode {
            RoomMode::Generated => self.catalogue_entry(selection).map_or_else(
                || format!("missing:{}:{}", tier_number(selection.tier), selection.seed),
                |entry| format!("catalogue:{}", entry.id()),
            ),
            RoomMode::DeveloperGenerated => format!(
                "developer-v6:{}:{:016x}",
                tier_number(selection.tier),
                selection.seed
            ),
            RoomMode::Development => format!("development:{}", tier_number(selection.tier)),
            RoomMode::Gallery => self.gallery_entry(selection).map_or_else(
                || format!("gallery:missing:{:016x}", selection.seed),
                |level| format!("gallery:{}", level.id()),
            ),
            RoomMode::CalibratedGenerated => format!(
                "calibrated-wall-jump-v{}:{:016x}",
                CALIBRATED_WALL_JUMP_GENERATION_VERSION, selection.seed
            ),
            RoomMode::Dungeon => self.dungeon_run.map_or_else(
                || "dungeon:missing".to_owned(),
                |run| format!("dungeon:{}", run.room.id()),
            ),
            RoomMode::DungeonV2 => self.dungeon_v2_run.as_ref().map_or_else(
                || "dungeon-v2".to_owned(),
                |run| format!("dungeon-v2:{}", run.room),
            ),
            RoomMode::Challenge(kind) => kind.stats_key(),
        }
    }

    fn catalogue_entry(&self, selection: ScenarioSelection) -> Option<&PlayableEntry> {
        (selection.mode == RoomMode::Generated)
            .then(|| {
                self.catalogue
                    .entry(selection.tier, selection.seed as usize)
            })
            .flatten()
    }

    fn current_entry(&self) -> Option<&PlayableEntry> {
        self.catalogue_entry(self.selection)
    }

    fn gallery_entry(&self, selection: ScenarioSelection) -> Option<CalibrationLevel> {
        if selection.mode != RoomMode::Gallery {
            return None;
        }
        usize::try_from(selection.seed)
            .ok()
            .and_then(|index| calibration_gallery().get(index))
            .copied()
    }

    fn current_gallery_entry(&self) -> Option<CalibrationLevel> {
        self.gallery_entry(self.selection)
    }

    fn calibrated_entry(
        &self,
        selection: ScenarioSelection,
    ) -> Option<CalibratedGeneratorPlaytestLevel> {
        if selection.mode != RoomMode::CalibratedGenerated {
            return None;
        }
        usize::try_from(selection.seed)
            .ok()
            .and_then(|index| calibrated_generator_playtest().get(index))
            .copied()
    }

    fn current_calibrated_entry(&self) -> Option<CalibratedGeneratorPlaytestLevel> {
        self.calibrated_entry(self.selection)
    }

    fn is_movement_course(&self) -> bool {
        is_movement_course_room(self.simulation.room())
    }

    fn open_movement_tuning_menu(&mut self) -> bool {
        if !self.human_controlled() {
            return false;
        }
        self.movement_tuning_menu = Some(MovementTuningMenuState {
            selected_row: 0,
            original: self.movement_tuning,
        });
        true
    }

    const fn movement_tuning_menu_visible(&self) -> bool {
        self.movement_tuning_menu.is_some()
    }

    fn move_movement_tuning_selection(&mut self, delta: isize) {
        let Some(menu) = &mut self.movement_tuning_menu else {
            return;
        };
        menu.selected_row = menu
            .selected_row
            .saturating_add_signed(delta)
            .min(MOVEMENT_TUNING_ROW_COUNT - 1);
    }

    fn adjust_movement_tuning(&mut self, direction: i16) {
        let Some(menu) = self.movement_tuning_menu else {
            return;
        };
        match menu.selected_row {
            0 => {
                self.movement_tuning.top_speed_pixels_per_second = adjust_tuning_value(
                    self.movement_tuning.top_speed_pixels_per_second,
                    direction * 5,
                    60,
                    180,
                );
            }
            1 => {
                self.movement_tuning.acceleration_milliseconds = adjust_tuning_value(
                    self.movement_tuning.acceleration_milliseconds,
                    direction * 5,
                    20,
                    300,
                );
            }
            2 => {
                self.movement_tuning.braking_milliseconds = adjust_tuning_value(
                    self.movement_tuning.braking_milliseconds,
                    direction * 5,
                    20,
                    500,
                );
            }
            3 => {
                self.movement_tuning.wall_ascent_carry_percent = adjust_tuning_value(
                    self.movement_tuning.wall_ascent_carry_percent,
                    direction * 10,
                    0,
                    150,
                );
            }
            4 => {
                self.movement_tuning.wall_carry_percent = adjust_tuning_value(
                    self.movement_tuning.wall_carry_percent,
                    direction * 10,
                    0,
                    150,
                );
            }
            5 => {
                self.movement_tuning.wall_momentum_milliseconds = adjust_tuning_value(
                    self.movement_tuning.wall_momentum_milliseconds,
                    direction * 25,
                    0,
                    500,
                );
            }
            _ => unreachable!("movement tuning menu row is clamped"),
        }
    }

    fn reset_movement_tuning_draft(&mut self) {
        if self.movement_tuning_menu.is_some() {
            self.movement_tuning = MovementTuning::GAMEPLAY_DEFAULT;
        }
    }

    fn cancel_movement_tuning_menu(&mut self) {
        if let Some(menu) = self.movement_tuning_menu.take() {
            self.movement_tuning = menu.original;
        }
    }

    fn close_movement_tuning_menu(&mut self) {
        let Some(menu) = self.movement_tuning_menu.take() else {
            return;
        };
        if menu.original == self.movement_tuning {
            return;
        }

        // Persist the old-policy attempt before changing a digest-affecting mechanic.
        let updated_tuning = self.movement_tuning;
        self.movement_tuning = menu.original;
        self.step_human(Action {
            restart: true,
            ..Action::default()
        });
        self.movement_tuning = updated_tuning;
        self.simulation.set_movement_tuning(updated_tuning);
        self.human_recorder.resume_at(&self.simulation);
        self.feedback = SimulationFeedback::default();

        let level_id = self.stats_key(self.selection);
        let level_name = self.level_name(self.selection);
        let history_error = self.human_history.as_mut().and_then(|history| {
            history
                .append_movement_tuning_change(&level_id, &level_name, self.movement_tuning)
                .err()
        });
        if let Some(error) = history_error {
            eprintln!("human attempt history disabled after write failure: {error}");
            self.human_history = None;
        }
        self.replay_notice = Some(ReplayNotice {
            title: "MOVEMENT TUNING APPLIED".to_owned(),
            detail: movement_tuning_summary(self.movement_tuning),
            is_error: false,
        });
    }

    fn selected_route_complete(&self) -> bool {
        if self.selection.mode == RoomMode::Dungeon
            && self.dungeon_run.is_some_and(|run| run.inventory.crown)
        {
            return true;
        }
        if let Some(kind) = self.selection.mode.challenge() {
            // Floor labs never declare completion: their analysis target is a
            // study coordinate, and rooms are left through their doors.
            if matches!(kind, ChallengeKind::DungeonFloor(_)) {
                return false;
            }
            return kind.objective().is_satisfied_by(&self.simulation);
        }
        match self.selected_target_id() {
            Some(target) => self.simulation.reached_exit() == Some(target),
            None => self.simulation.reached_exit().is_some(),
        }
    }

    fn selected_target_id(&self) -> Option<&str> {
        self.current_entry()
            .map(PlayableEntry::target_door_id)
            .or_else(|| self.current_gallery_entry().map(CalibrationLevel::target))
            .or_else(|| self.selection.mode.challenge().map(ChallengeKind::target))
            .or_else(|| {
                (self.selection.mode == RoomMode::Dungeon).then_some(DEMO_DUNGEON_GOAL_EXIT)
            })
    }

    fn wrong_door(&self) -> Option<&str> {
        let reached = self.simulation.reached_exit()?;
        let target = self.selected_target_id()?;
        (reached != target).then_some(reached)
    }

    fn acknowledge_menu_input_release(&mut self, navigation_held: bool) -> bool {
        if !self.awaiting_menu_input_release {
            return true;
        }
        if navigation_held {
            return false;
        }
        self.awaiting_menu_input_release = false;
        true
    }

    const fn human_controlled(&self) -> bool {
        matches!(&self.replay_mode, ReplayMode::Human)
    }

    fn observe_human_jump_frame(&mut self, sample: RawJumpInputSample) {
        self.human_recorder.observe_jump_frame(sample);
    }

    const fn solve_requested(&self) -> bool {
        matches!(&self.replay_mode, ReplayMode::SolveRequested(_))
    }

    fn replay_paused(&self) -> bool {
        matches!(
            &self.replay_mode,
            ReplayMode::Playback(playback) if playback.transport.phase == PlaybackPhase::Paused
        )
    }

    fn replay_complete(&self) -> bool {
        matches!(
            &self.replay_mode,
            ReplayMode::Playback(playback) if playback.transport.phase == PlaybackPhase::Complete
        )
    }

    fn request_solve(&mut self) {
        self.request_solver(SolveRequest::Route);
    }

    fn request_pickup_solve(&mut self) {
        let pickup_id = match self.pickup_solve_target() {
            Ok(pickup_id) => pickup_id,
            Err(error) => {
                self.reject_pickup_solve(error);
                return;
            }
        };
        self.request_solver(SolveRequest::Pickup(pickup_id));
    }

    fn pickup_solve_target(&self) -> Result<String, PickupSolveRequestError> {
        if let Some(pickup) = self
            .current_entry()
            .and_then(PlayableEntry::representative_pickup)
        {
            return Ok(pickup.pickup_id().to_owned());
        }
        match self.simulation.room().pickups() {
            [] => Err(PickupSolveRequestError::NoPickups),
            [pickup] => Ok(pickup.id().to_owned()),
            pickups => Err(PickupSolveRequestError::MultiplePickups(pickups.len())),
        }
    }

    fn reject_pickup_solve(&mut self, error: PickupSolveRequestError) {
        let resume_recording = matches!(&self.replay_mode, ReplayMode::Playback(_));
        self.replay_mode = ReplayMode::Human;
        if resume_recording {
            self.human_recorder.resume_at(&self.simulation);
        }
        let (title, detail) = match error {
            PickupSolveRequestError::NoPickups => (
                "NO COIN IN THIS ROOM".to_owned(),
                "C is available only when a room has exactly one coin".to_owned(),
            ),
            PickupSolveRequestError::MultiplePickups(count) => (
                "CHOOSE A COIN".to_owned(),
                format!("this room has {count} coins; targeted selection is not in the lab yet"),
            ),
        };
        self.replay_notice = Some(ReplayNotice {
            title,
            detail,
            is_error: true,
        });
    }

    fn request_solver(&mut self, request: SolveRequest) {
        if matches!(&self.replay_mode, ReplayMode::Playback(_)) {
            self.human_recorder.resume_at(&self.simulation);
        }
        self.replay_mode = ReplayMode::SolveRequested(request);
        self.replay_notice = None;
    }

    fn cancel_replay(&mut self) -> bool {
        let (detail, resume_recording) = match &self.replay_mode {
            ReplayMode::Human => return false,
            ReplayMode::SolveRequested(_) => ("solver request cancelled".to_owned(), false),
            ReplayMode::Playback(playback) => (
                format!(
                    "human control restored at frame {}/{}",
                    playback.transport.next_frame, playback.transport.total_frames
                ),
                true,
            ),
        };
        self.replay_mode = ReplayMode::Human;
        if resume_recording {
            self.human_recorder.resume_at(&self.simulation);
        }
        self.replay_notice = Some(ReplayNotice {
            title: "REPLAY CANCELLED".to_owned(),
            detail,
            is_error: false,
        });
        true
    }

    fn start_last_human_replay(&mut self) {
        let Some(attempt) = self.human_recorder.preferred().cloned() else {
            self.replay_notice = Some(ReplayNotice {
                title: "NO HUMAN ATTEMPT".to_owned(),
                detail: "finish an attempt with death, reset, or an exit first".to_owned(),
                is_error: true,
            });
            return;
        };
        let verification = match attempt.replay.verify(&attempt.initial) {
            Ok(verification) => verification,
            Err(error) => {
                self.set_replay_error("HUMAN REPLAY ERROR", error.to_string());
                return;
            }
        };
        if let Some(expected_exit) = attempt.outcome.expected_exit()
            && verification.reached_exit.as_deref() != Some(expected_exit)
        {
            self.set_replay_error(
                "HUMAN REPLAY ERROR",
                format!(
                    "expected exit {expected_exit:?}, reached {:?}",
                    verification.reached_exit
                ),
            );
            return;
        }

        let expected_objective = attempt
            .outcome
            .expected_exit()
            .map(|id| ReplayObjective::Exit(id.to_owned()));
        let transport = PlaybackTransport::new(attempt.replay.frames.len());
        self.simulation = attempt.initial.clone();
        self.feedback = SimulationFeedback::default();
        self.human_recorder.suspend();
        self.replay_notice = None;
        self.replay_mode = ReplayMode::Playback(Box::new(ReplayPlayback {
            initial: attempt.initial,
            replay: attempt.replay,
            transport,
            expected_objective,
            origin: ReplayOrigin::Human(attempt.outcome),
        }));
    }

    fn toggle_replay_playback(&mut self) -> bool {
        let initial_to_restore = match &mut self.replay_mode {
            ReplayMode::Playback(playback) => {
                let was_complete = playback.transport.phase == PlaybackPhase::Complete;
                if !playback.transport.toggle_play_pause() {
                    return false;
                }
                was_complete.then(|| playback.initial.clone())
            }
            ReplayMode::Human | ReplayMode::SolveRequested(_) => return false,
        };
        if let Some(initial) = initial_to_restore {
            self.simulation = initial;
            self.feedback = SimulationFeedback::default();
        }
        true
    }

    fn perform_requested_solve(&mut self) {
        let request = match &self.replay_mode {
            ReplayMode::SolveRequested(request) => request.clone(),
            ReplayMode::Human | ReplayMode::Playback(_) => return,
        };
        if self.selection.mode == RoomMode::Dungeon {
            self.perform_dungeon_solve(request);
            return;
        }
        if self.selection.mode == RoomMode::DungeonV2 {
            self.perform_dungeon_v2_solve(request);
            return;
        }
        let (mut initial, generated_provenance) =
            match load_scenario(self.selection, &self.catalogue) {
                Ok(scenario) => scenario,
                Err(error) => {
                    self.set_replay_error("SCENARIO ERROR", error);
                    return;
                }
            };
        configure_live_simulation(&mut initial, self.movement_tuning);
        match request {
            SolveRequest::Route => {
                if let Some(kind) = self.selection.mode.challenge() {
                    self.perform_challenge_route(kind, initial, generated_provenance);
                } else if let Some(level) = self.current_calibrated_entry() {
                    self.perform_calibrated_route(level, initial, generated_provenance);
                } else if let Some(level) = self.current_gallery_entry() {
                    self.perform_gallery_route(level, initial, generated_provenance);
                } else if self.current_entry().is_some() {
                    self.perform_catalogue_route(initial, generated_provenance);
                } else {
                    self.perform_exit_solve(initial, generated_provenance);
                }
            }
            SolveRequest::Pickup(pickup_id) => {
                if !self.perform_catalogue_pickup(&initial, generated_provenance, &pickup_id) {
                    self.perform_pickup_solve(initial, generated_provenance, pickup_id);
                }
            }
        }
    }

    fn perform_dungeon_solve(&mut self, request: SolveRequest) {
        let Some(run) = self.dungeon_run else {
            self.set_replay_error(
                "DUNGEON STATE ERROR",
                "persistent run state is missing".to_owned(),
            );
            return;
        };
        let initial = self.simulation.clone();
        if let SolveRequest::Pickup(pickup_id) = request {
            self.perform_pickup_solve(initial, None, pickup_id);
            return;
        }
        if run.room == DemoDungeonRoom::ClimberVault && !run.inventory.climbing_gloves {
            self.perform_pickup_solve(initial, None, DEMO_DUNGEON_GLOVE_PICKUP.to_owned());
            return;
        }
        if run.room == DemoDungeonRoom::BootsVault && !run.inventory.winged_boots {
            self.perform_pickup_solve(initial, None, DEMO_DUNGEON_BOOT_PICKUP.to_owned());
            return;
        }
        if let Some(coin_id) = self
            .simulation
            .room()
            .pickups()
            .iter()
            .enumerate()
            .find(|(index, pickup)| {
                pickup.id().starts_with("dungeon-coin-")
                    && self.simulation.pickup_is_collected(*index) == Some(false)
            })
            .map(|(_, pickup)| pickup.id().to_owned())
        {
            self.perform_pickup_solve(initial, None, coin_id);
            return;
        }
        // The Crown goal is the only terminal exit; everything else picks the
        // nearest openable door that is not the one the player entered by
        // (the entry door is a last resort so retreat is still offered).
        if run.room == DemoDungeonRoom::CrownSanctum {
            self.perform_exit_solve(initial, None);
            return;
        }
        let entry_door = self.simulation.entry_door().map(str::to_owned);
        let player = self.simulation.player().bounds();
        let progression = run.inventory.authored_progression_inventory();
        let mut candidates: Vec<(i64, String)> = self
            .simulation
            .room()
            .doors()
            .iter()
            .filter(|door| {
                demo_dungeon_door_requirement(run.room, &door.id).is_satisfied_by(&progression)
            })
            .map(|door| {
                let trigger = door.trigger_bounds;
                let dx = i64::from(trigger.x + trigger.width / 2 - player.x - player.width / 2);
                let dy = i64::from(trigger.y + trigger.height / 2 - player.y - player.height / 2);
                let mut score = dx * dx + dy * dy;
                if Some(door.id.as_str()) == entry_door.as_deref() {
                    score += 1_000_000_000;
                }
                (score, door.id.clone())
            })
            .collect();
        candidates.sort();
        let target_door = candidates.into_iter().next().map(|(_, id)| id);
        if let Some(target) = target_door {
            self.perform_target_door_solve(initial, None, target, None, 0, 0);
        } else {
            self.perform_exit_solve(initial, None);
        }
    }

    fn perform_dungeon_v2_solve(&mut self, request: SolveRequest) {
        let Some(run) = self.dungeon_v2_run.clone() else {
            self.set_replay_error(
                "DUNGEON STATE ERROR",
                "persistent v2 run state is missing".to_owned(),
            );
            return;
        };
        let initial = self.simulation.clone();
        if let SolveRequest::Pickup(pickup_id) = request {
            self.perform_pickup_solve(initial, None, pickup_id);
            return;
        }
        let dungeon = dungeon_v2_definition();
        if run.room == dungeon.glove_room && !run.inventory.climbing_gloves {
            self.perform_pickup_solve(initial, None, DEMO_DUNGEON_GLOVE_PICKUP.to_owned());
            return;
        }
        if run.room == dungeon.boots_room && !run.inventory.winged_boots {
            self.perform_pickup_solve(initial, None, DEMO_DUNGEON_BOOT_PICKUP.to_owned());
            return;
        }
        if run.room == dungeon.goal && !run.inventory.crown {
            self.perform_pickup_solve(initial, None, DEMO_DUNGEON_CROWN_PICKUP.to_owned());
            return;
        }
        if run.room == dungeon.spawn && run.inventory.crown {
            // With the crown held the spawn room hosts the escape gate.
            self.perform_exit_solve(initial, None);
            return;
        }
        if let Some(coin_id) = self
            .simulation
            .room()
            .pickups()
            .iter()
            .enumerate()
            .find(|(index, pickup)| {
                pickup.id().starts_with("v2-coin-")
                    && self.simulation.pickup_is_collected(*index) == Some(false)
            })
            .map(|(_, pickup)| pickup.id().to_owned())
        {
            self.perform_pickup_solve(initial, None, coin_id);
            return;
        }
        // Nearest openable door that is not the entry door (which stays as a
        // last resort so retreat is still offered).
        let entry_door = self.simulation.entry_door().map(str::to_owned);
        let player = self.simulation.player().bounds();
        let mut candidates: Vec<(i64, String)> = self
            .simulation
            .room()
            .doors()
            .iter()
            .filter(|door| {
                dungeon_v2_door_requirement(&run.room, &door.id)
                    .is_none_or(|requirement| run.inventory.satisfies(requirement))
            })
            .map(|door| {
                let trigger = door.trigger_bounds;
                let dx = i64::from(trigger.x + trigger.width / 2 - player.x - player.width / 2);
                let dy = i64::from(trigger.y + trigger.height / 2 - player.y - player.height / 2);
                let mut score = dx * dx + dy * dy;
                if Some(door.id.as_str()) == entry_door.as_deref() {
                    score += 1_000_000_000;
                }
                (score, door.id.clone())
            })
            .collect();
        candidates.sort();
        if let Some((_, target)) = candidates.into_iter().next() {
            self.perform_target_door_solve(initial, None, target, None, 0, 0);
        } else {
            self.perform_exit_solve(initial, None);
        }
    }

    fn perform_challenge_route(
        &mut self,
        kind: ChallengeKind,
        initial: Simulation,
        generated_provenance: Option<GeneratedProvenance>,
    ) {
        let objective = kind.objective();
        let replay = Replay::record(&initial, kind.witness_actions());
        let verification = match replay.verify(&initial) {
            Ok(verification) => verification,
            Err(error) => {
                self.set_replay_error("CHALLENGE VERIFY ERROR", error.to_string());
                return;
            }
        };
        if !objective.is_satisfied_by_verification(&verification) {
            self.set_replay_error(
                "CHALLENGE VERIFY ERROR",
                format!(
                    "stored route expected {}, but its final state did not satisfy it",
                    objective.status_label()
                ),
            );
            return;
        }
        self.install_playback(
            initial,
            generated_provenance,
            replay,
            objective,
            ReplayOrigin::Challenge(kind),
        );
    }

    fn perform_gallery_route(
        &mut self,
        level: CalibrationLevel,
        initial: Simulation,
        generated_provenance: Option<GeneratedProvenance>,
    ) {
        let replay = Replay::record(&initial, level.witness_actions());
        let verification = match replay.verify(&initial) {
            Ok(verification) => verification,
            Err(error) => {
                self.set_replay_error("GALLERY VERIFY ERROR", error.to_string());
                return;
            }
        };
        if verification.reached_exit.as_deref() != Some(level.target()) {
            self.set_replay_error(
                "GALLERY VERIFY ERROR",
                format!(
                    "{} stored route expected exit {:?}, reached {:?}",
                    level.id(),
                    level.target(),
                    verification.reached_exit
                ),
            );
            return;
        }
        self.install_playback(
            initial,
            generated_provenance,
            replay,
            ReplayObjective::Exit(level.target().to_owned()),
            ReplayOrigin::Gallery(level),
        );
    }

    fn perform_calibrated_route(
        &mut self,
        level: CalibratedGeneratorPlaytestLevel,
        initial: Simulation,
        generated_provenance: Option<GeneratedProvenance>,
    ) {
        let replay = Replay::record(&initial, level.witness_actions());
        let verification = match replay.verify(&initial) {
            Ok(verification) => verification,
            Err(error) => {
                self.set_replay_error("GENERATED VERIFY ERROR", error.to_string());
                return;
            }
        };
        if verification.reached_exit.as_deref() != Some(level.target()) {
            self.set_replay_error(
                "GENERATED VERIFY ERROR",
                format!(
                    "seed {} expected exit {:?}, reached {:?}",
                    level.seed(),
                    level.target(),
                    verification.reached_exit
                ),
            );
            return;
        }
        self.install_playback(
            initial,
            generated_provenance,
            replay,
            ReplayObjective::Exit(level.target().to_owned()),
            ReplayOrigin::CalibratedGenerated(level),
        );
    }

    fn perform_catalogue_route(
        &mut self,
        initial: Simulation,
        generated_provenance: Option<GeneratedProvenance>,
    ) {
        let Some(entry) = self.current_entry() else {
            self.set_replay_error("CATALOGUE ERROR", "selected route is missing".to_owned());
            return;
        };
        let target = entry.target_door_id().to_owned();
        let band = entry.legacy().map(|entry| entry.band());
        let successful_wall_jumps = entry
            .legacy()
            .map_or(0, |entry| entry.successful_wall_jumps());
        let successful_dashes = entry.legacy().map_or(0, |entry| entry.successful_dashes());
        let witness_matches_loadout = entry.corpus().is_none_or(|corpus| {
            corpus.key().construction_loadout().abilities() == initial.abilities()
        });
        let stored_actions = (PLAYER_MOVEMENT_POLICY_VERSION
            == HISTORICAL_CATALOGUE_WITNESS_MOVEMENT_POLICY_VERSION
            && witness_matches_loadout)
            .then(|| entry.representative_actions())
            .flatten()
            .map(|actions| actions.actions().collect::<Vec<_>>());

        if let Some(actions) = stored_actions {
            let replay = Replay::record(&initial, actions);
            let verification = match replay.verify(&initial) {
                Ok(verification) => verification,
                Err(error) => {
                    self.set_replay_error("CATALOGUE VERIFY ERROR", error.to_string());
                    return;
                }
            };
            if verification.reached_exit.as_deref() != Some(target.as_str()) {
                self.set_replay_error(
                    "CATALOGUE VERIFY ERROR",
                    format!(
                        "stored route expected door {target:?}, reached {:?}",
                        verification.reached_exit
                    ),
                );
                return;
            }
            self.install_playback(
                initial,
                generated_provenance,
                replay,
                ReplayObjective::Door(target),
                ReplayOrigin::CatalogueRoute {
                    band,
                    stored_witness: true,
                    successful_wall_jumps,
                    successful_dashes,
                    search_stats: None,
                },
            );
            return;
        }

        self.perform_target_door_solve(
            initial,
            generated_provenance,
            target,
            band,
            successful_wall_jumps,
            successful_dashes,
        );
    }

    fn perform_target_door_solve(
        &mut self,
        initial: Simulation,
        generated_provenance: Option<GeneratedProvenance>,
        target: String,
        band: Option<CatalogueBand>,
        successful_wall_jumps: usize,
        successful_dashes: usize,
    ) {
        let config = SolverConfig::for_abilities(initial.abilities());
        let outcome = match solve_target(&initial, SearchTarget::door(target.clone()), &config) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.set_replay_error("ROUTE SOLVER ERROR", error.to_string());
                return;
            }
        };
        let TargetSolveOutcome::Solved(solution) = outcome else {
            let TargetSolveOutcome::Inconclusive { reason, stats } = outcome else {
                unreachable!("all targeted solve outcomes handled")
            };
            self.replay_mode = ReplayMode::Human;
            self.replay_notice = Some(ReplayNotice {
                title: "ROUTE AI INCONCLUSIVE".to_owned(),
                detail: format!(
                    "{} / nodes {} / sim ticks {}",
                    inconclusive_reason_label(reason),
                    stats.expanded_nodes,
                    stats.simulated_ticks
                ),
                is_error: true,
            });
            return;
        };
        if solution.reached != ReachedTarget::Door(target.clone()) {
            self.set_replay_error(
                "ROUTE VERIFY ERROR",
                format!("targeted door {target:?}, reached {:?}", solution.reached),
            );
            return;
        }
        let search_stats = solution.stats;
        self.install_playback(
            initial,
            generated_provenance,
            solution.replay,
            ReplayObjective::Door(target),
            ReplayOrigin::CatalogueRoute {
                band,
                stored_witness: false,
                successful_wall_jumps,
                successful_dashes,
                search_stats: Some(search_stats),
            },
        );
    }

    fn perform_catalogue_pickup(
        &mut self,
        initial: &Simulation,
        generated_provenance: Option<GeneratedProvenance>,
        pickup_id: &str,
    ) -> bool {
        let stored = (PLAYER_MOVEMENT_POLICY_VERSION
            == HISTORICAL_CATALOGUE_WITNESS_MOVEMENT_POLICY_VERSION)
            .then(|| self.current_entry())
            .flatten()
            .and_then(|entry| {
                let pickup = entry.representative_pickup()?;
                (pickup.pickup_id() == pickup_id
                    && pickup.source_door_id() == entry.source_door_id())
                .then(|| {
                    (
                        pickup.pickup_id().to_owned(),
                        pickup.actions().actions().collect::<Vec<_>>(),
                    )
                })
            });
        let Some((pickup_id, actions)) = stored else {
            return false;
        };
        let replay = Replay::record(initial, actions);
        let verification = match replay.verify(initial) {
            Ok(verification) => verification,
            Err(error) => {
                self.set_replay_error("COIN VERIFY ERROR", error.to_string());
                return true;
            }
        };
        if !verification
            .collected_pickup_ids
            .iter()
            .any(|collected| collected == &pickup_id)
        {
            self.set_replay_error(
                "COIN VERIFY ERROR",
                format!("stored route did not collect {pickup_id:?}"),
            );
            return true;
        }
        self.install_playback(
            initial.clone(),
            generated_provenance,
            replay,
            ReplayObjective::Pickup(pickup_id.clone()),
            ReplayOrigin::PickupSolver {
                pickup_id,
                stored_witness: true,
                search_stats: None,
            },
        );
        true
    }

    fn install_playback(
        &mut self,
        initial: Simulation,
        generated_provenance: Option<GeneratedProvenance>,
        replay: Replay,
        objective: ReplayObjective,
        origin: ReplayOrigin,
    ) {
        let transport = PlaybackTransport::new(replay.frames.len());
        self.simulation = initial.clone();
        self.generated_provenance = generated_provenance;
        self.feedback = SimulationFeedback::default();
        self.human_recorder.suspend();
        self.replay_notice = None;
        self.replay_mode = ReplayMode::Playback(Box::new(ReplayPlayback {
            initial,
            replay,
            transport,
            expected_objective: Some(objective),
            origin,
        }));
    }

    fn perform_exit_solve(
        &mut self,
        initial: Simulation,
        generated_provenance: Option<GeneratedProvenance>,
    ) {
        let config = SolverConfig::for_abilities(initial.abilities());
        let outcome = match solve(&initial, &config) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.set_replay_error("SOLVER ERROR", error.to_string());
                return;
            }
        };
        let SolveOutcome::Solved(solution) = outcome else {
            let SolveOutcome::Inconclusive { reason, stats } = outcome else {
                unreachable!("all solve outcomes handled")
            };
            self.replay_mode = ReplayMode::Human;
            self.replay_notice = Some(ReplayNotice {
                title: "AI INCONCLUSIVE".to_owned(),
                detail: format!(
                    "{} / nodes {} / sim ticks {}",
                    inconclusive_reason_label(reason),
                    stats.expanded_nodes,
                    stats.simulated_ticks
                ),
                is_error: true,
            });
            return;
        };

        let verification = match solution.replay.verify(&initial) {
            Ok(verification) => verification,
            Err(error) => {
                self.set_replay_error("VERIFY ERROR", error.to_string());
                return;
            }
        };
        if verification.reached_exit.as_deref() != Some(solution.exit_id.as_str()) {
            self.set_replay_error(
                "VERIFY ERROR",
                format!(
                    "witness expected exit {:?}, reached {:?}",
                    solution.exit_id, verification.reached_exit
                ),
            );
            return;
        }
        let difficulty = match analyze_solution(&initial, &solution, &DifficultyConfig::default()) {
            Ok(report) => report,
            Err(error) => {
                self.set_replay_error("ANALYSIS ERROR", error.to_string());
                return;
            }
        };
        let diagnostics = SolverDiagnostics {
            search_stats: solution.stats,
            provisional_band: difficulty.provisional_complexity.band,
            temporal_robustness: difficulty.temporal_robustness.successful_perturbation_ratio,
            accepted_wall_jumps: difficulty.successful_wall_jumps,
            accepted_dashes: difficulty.successful_dashes,
        };

        let transport = PlaybackTransport::new(solution.replay.frames.len());
        self.simulation = initial.clone();
        self.generated_provenance = generated_provenance;
        self.feedback = SimulationFeedback::default();
        self.human_recorder.suspend();
        self.replay_notice = None;
        self.replay_mode = ReplayMode::Playback(Box::new(ReplayPlayback {
            initial,
            replay: solution.replay,
            transport,
            expected_objective: Some(ReplayObjective::Exit(solution.exit_id)),
            origin: ReplayOrigin::Solver(diagnostics),
        }));
    }

    fn perform_pickup_solve(
        &mut self,
        initial: Simulation,
        generated_provenance: Option<GeneratedProvenance>,
        pickup_id: String,
    ) {
        let config = SolverConfig::for_abilities(initial.abilities());
        let outcome = match solve_target(&initial, SearchTarget::pickup(pickup_id.clone()), &config)
        {
            Ok(outcome) => outcome,
            Err(error) => {
                self.set_replay_error("COIN SOLVER ERROR", error.to_string());
                return;
            }
        };
        let TargetSolveOutcome::Solved(solution) = outcome else {
            let TargetSolveOutcome::Inconclusive { reason, stats } = outcome else {
                unreachable!("all targeted solve outcomes handled")
            };
            self.replay_mode = ReplayMode::Human;
            self.replay_notice = Some(ReplayNotice {
                title: "COIN AI INCONCLUSIVE".to_owned(),
                detail: format!(
                    "{} / nodes {} / sim ticks {}",
                    inconclusive_reason_label(reason),
                    stats.expanded_nodes,
                    stats.simulated_ticks
                ),
                is_error: true,
            });
            return;
        };

        if solution.reached != ReachedTarget::Pickup(pickup_id.clone()) {
            self.set_replay_error(
                "COIN VERIFY ERROR",
                format!(
                    "witness targeted pickup {pickup_id:?}, reached {:?}",
                    solution.reached
                ),
            );
            return;
        }
        let verification = match solution.replay.verify(&initial) {
            Ok(verification) => verification,
            Err(error) => {
                self.set_replay_error("COIN VERIFY ERROR", error.to_string());
                return;
            }
        };
        if !verification
            .collected_pickup_ids
            .iter()
            .any(|id| id == &pickup_id)
        {
            self.set_replay_error(
                "COIN VERIFY ERROR",
                format!(
                    "witness expected pickup {pickup_id:?}, collected {:?}",
                    verification.collected_pickup_ids
                ),
            );
            return;
        }

        let search_stats = solution.stats;
        let transport = PlaybackTransport::new(solution.replay.frames.len());
        self.simulation = initial.clone();
        self.generated_provenance = generated_provenance;
        self.feedback = SimulationFeedback::default();
        self.human_recorder.suspend();
        self.replay_notice = None;
        self.replay_mode = ReplayMode::Playback(Box::new(ReplayPlayback {
            initial,
            replay: solution.replay,
            transport,
            expected_objective: Some(ReplayObjective::Pickup(pickup_id.clone())),
            origin: ReplayOrigin::PickupSolver {
                pickup_id,
                stored_witness: false,
                search_stats: Some(search_stats),
            },
        }));
    }

    fn advance_replay(&mut self, frame_step: bool) {
        let (frame_index, frame) = match &self.replay_mode {
            ReplayMode::Playback(playback) if playback.transport.should_advance(frame_step) => {
                let index = playback.transport.next_frame;
                (index, playback.replay.frames[index])
            }
            ReplayMode::Human | ReplayMode::SolveRequested(_) | ReplayMode::Playback(_) => return,
        };

        // Replays deliberately use the exact same step and feedback path as human input.
        let report = self.step(frame.action);
        if report.digest != frame.expected_digest {
            self.set_replay_error(
                "PLAYBACK DIVERGED",
                format!(
                    "frame {frame_index}: expected {}, got {}",
                    frame.expected_digest, report.digest
                ),
            );
            return;
        }
        let actual_event_digest = digest_events(&report.events);
        if actual_event_digest != frame.expected_event_digest {
            self.set_replay_error(
                "PLAYBACK DIVERGED",
                format!(
                    "frame {frame_index}: expected event digest {}, got {} from {:?}",
                    frame.expected_event_digest, actual_event_digest, report.events
                ),
            );
            return;
        }

        let (complete, completed_objective) = match &mut self.replay_mode {
            ReplayMode::Playback(playback) => {
                playback.transport.mark_advanced();
                let complete = playback.transport.phase == PlaybackPhase::Complete;
                (
                    complete,
                    complete
                        .then(|| playback.expected_objective.clone())
                        .flatten(),
                )
            }
            ReplayMode::Human | ReplayMode::SolveRequested(_) => (false, None),
        };
        if let Some(expected_objective) = completed_objective
            && !expected_objective.is_satisfied_by(&self.simulation)
        {
            self.set_replay_error(
                "PLAYBACK FAILED",
                format!(
                    "expected {}, but the final state did not satisfy it",
                    expected_objective.status_label()
                ),
            );
            return;
        }
        // AI progress is as real as human progress: persist coins and follow
        // door transitions, and hand control back the moment a goal lands.
        let boundary = self.observe_dungeon_progress(&report)
            || self.observe_dungeon_v2_progress(&report)
            || self.observe_floor_lab_progress(&report);
        if boundary {
            self.replay_mode = ReplayMode::Human;
            self.human_recorder = HumanRecorder::new(&self.simulation);
        } else if complete
            && matches!(&self.replay_mode, ReplayMode::Playback(_))
            && (matches!(self.selection.mode, RoomMode::Dungeon | RoomMode::DungeonV2)
                || matches!(
                    self.selection.mode,
                    RoomMode::Challenge(ChallengeKind::DungeonFloor(_))
                ))
        {
            // In the dungeon and the floor lab, a finished AI goal hands
            // control straight back; review modes keep the replay transport.
            self.replay_mode = ReplayMode::Human;
            self.human_recorder.resume_at(&self.simulation);
            self.replay_notice = Some(ReplayNotice {
                title: "AI GOAL COMPLETE".to_owned(),
                detail: "control returned; V solves the next goal".to_owned(),
                is_error: false,
            });
        }
    }

    fn set_replay_error(&mut self, title: &str, detail: String) {
        eprintln!("{title}: {detail}");
        let resume_recording = matches!(&self.replay_mode, ReplayMode::Playback(_));
        self.replay_mode = ReplayMode::Human;
        if resume_recording {
            self.human_recorder.resume_at(&self.simulation);
        }
        self.replay_notice = Some(ReplayNotice {
            title: title.to_owned(),
            detail,
            is_error: true,
        });
    }

    fn step(&mut self, action: Action) -> StepReport {
        // Notices live on the HUD rail and expire on their own so gameplay is
        // never obscured; errors linger a little longer.
        match &self.replay_notice {
            Some(notice) => {
                let key = (notice.title.clone(), notice.detail.clone());
                if self.notice_seen.as_ref() == Some(&key) {
                    self.notice_age += 1;
                    let lifetime = if notice.is_error { 600 } else { 300 };
                    if self.notice_age > lifetime {
                        self.replay_notice = None;
                    }
                } else {
                    self.notice_seen = Some(key);
                    self.notice_age = 0;
                }
            }
            None => self.notice_seen = None,
        }
        let velocity_x_before = self.simulation.player().velocity_subpixels().x;
        let grounded_before = self.simulation.player().grounded();
        self.feedback.advance_tick();
        let report = self.simulation.step(action);
        self.feedback.observe(&report.events);
        self.feedback.observe_motion(
            action,
            velocity_x_before,
            grounded_before,
            self.simulation.player().grounded(),
        );
        report
    }

    fn step_human(&mut self, action: Action) -> StepReport {
        // A short input lockout after dying keeps held movement from carrying
        // the respawned player straight back through the entry door. This is
        // presentation-side only: the authoritative simulation and recorded
        // replays are unaffected by how the client debounces human input.
        let action = if self.death_pause_ticks > 0 {
            self.death_pause_ticks -= 1;
            Action::default()
        } else {
            action
        };
        if !self.simulation.human_wall_assists_enabled()
            || self.simulation.movement_tuning() != self.movement_tuning
        {
            configure_live_simulation(&mut self.simulation, self.movement_tuning);
            // Start a fresh human-attempt boundary when returning to live play so the recorded
            // initial digest and subsequent frames share one movement-policy domain.
            self.human_recorder.resume_at(&self.simulation);
        }
        // Every connected door is a legitimate room-local completion in dungeon mode. The Crown
        // remains the run-level target, but classifying intermediate doors against that ID would
        // corrupt playtest history by recording every successful transition as a wrong door.
        let expected_target = (self.selection.mode != RoomMode::Dungeon)
            .then(|| self.selected_target_id().map(str::to_owned))
            .flatten();
        let report = self.step(action);
        if report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Died(_)))
        {
            self.death_pause_ticks = DEATH_PAUSE_TICKS;
        }
        let completed = self.human_recorder.observe_step(
            action,
            &report,
            &self.simulation,
            expected_target.as_deref(),
        );
        let attempt_ticks = completed.as_ref().and_then(|_| {
            self.human_recorder
                .last_completed
                .as_ref()
                .map(|attempt| attempt.replay.frames.len())
        });
        self.level_stats
            .entry(self.stats_key(self.selection))
            .or_default()
            .observe_human_step(&report.events, completed.as_ref(), attempt_ticks);
        let completed_notice = if let Some(outcome) = completed {
            let level_id = self.stats_key(self.selection);
            let level_name = self.level_name(self.selection);
            let history_error = self
                .human_history
                .as_mut()
                .zip(self.human_recorder.last_completed.as_ref())
                .and_then(|(history, attempt)| {
                    history
                        .append_attempt(&level_id, &level_name, attempt)
                        .err()
                });
            if let Some(error) = history_error {
                eprintln!("human attempt history disabled after write failure: {error}");
                self.human_history = None;
            }
            Some(match &outcome {
                AttemptOutcome::WrongDoor(id) => ReplayNotice {
                    title: "WRONG DOOR".to_owned(),
                    detail: format!("reached {id}; R retries the selected route, M opens levels"),
                    is_error: true,
                },
                _ => ReplayNotice {
                    title: format!("{} ATTEMPT RECORDED", outcome.label()),
                    detail: "H replays the latest successful or completed attempt".to_owned(),
                    is_error: false,
                },
            })
        } else {
            None
        };
        let dungeon_boundary = self.observe_dungeon_progress(&report)
            || self.observe_dungeon_v2_progress(&report)
            || self.observe_floor_lab_progress(&report);
        if !dungeon_boundary && let Some(notice) = completed_notice {
            self.replay_notice = Some(notice);
        }
        report
    }

    /// Apply the persistent run state that sits above an individual authoritative room.
    ///
    /// Returns `true` when this tick crossed an attempt boundary (a room transition or the boots
    /// unlock). The ordinary single-room recorder must not append across that digest/loadout
    /// boundary; it is resumed against the newly authoritative state here instead.
    fn persist_dungeon_progress(&mut self) {
        let result = match (self.dungeon_persistence.as_ref(), self.dungeon_run) {
            (Some(persistence), Some(run)) => {
                persistence.persist(run, self.simulation.entry_door(), &self.explored_rooms)
            }
            (None, _) | (_, None) => return,
        };
        if let Err(error) = result {
            eprintln!("dungeon persistence disabled after write failure: {error}");
            self.dungeon_persistence = None;
            self.replay_notice = Some(ReplayNotice {
                title: "DUNGEON SAVE FAILED".to_owned(),
                detail: "The current run continues, but progress is no longer being saved"
                    .to_owned(),
                is_error: true,
            });
        }
    }

    fn record_dungeon_event(
        &mut self,
        event: &str,
        room: DemoDungeonRoom,
        destination: Option<DemoDungeonRoom>,
        door: Option<&str>,
        item_id: Option<&str>,
        inventory: DemoDungeonInventory,
    ) {
        let result = self.human_history.as_mut().map(|history| {
            history.append_dungeon_event(
                DungeonEventContext {
                    event,
                    room,
                    destination,
                    door,
                    item_id,
                },
                inventory,
                self.movement_tuning,
            )
        });
        if let Some(Err(error)) = result {
            eprintln!("human attempt history disabled after dungeon-event failure: {error}");
            self.human_history = None;
        }
    }

    /// In the non-persistent floor lab, a reached connected door continues
    /// into the destination room's floor lab (entered via the mate door, with
    /// that room's analysis loadout) instead of ending the session.
    fn persist_dungeon_v2_progress(&mut self) {
        if let (Some(run), Some(persistence)) = (
            self.dungeon_v2_run.as_ref(),
            self.dungeon_v2_persistence.as_ref(),
        ) && let Err(error) =
            persistence.persist(run, self.simulation.entry_door(), &self.explored_v2_rooms)
        {
            eprintln!("dungeon v2 save failed: {error}");
        }
    }

    fn observe_dungeon_v2_progress(&mut self, report: &StepReport) -> bool {
        if let Some(live) = self.dungeon_v2_run.as_mut() {
            live.play_ticks = live.play_ticks.saturating_add(1);
            live.deaths = live.deaths.saturating_add(
                report
                    .events
                    .iter()
                    .filter(|event| matches!(event, SimulationEvent::Died(_)))
                    .count() as u32,
            );
        }
        let Some(mut run) = self.dungeon_v2_run.clone() else {
            return false;
        };

        // Dying after touching a persistent pickup restores it in the live
        // simulation, but coins already banked must stay gone: rebind the room
        // against the inventory on any reset.
        if report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Reset))
        {
            let entry_door = self.simulation.entry_door().map(str::to_owned);
            let room = dungeon_v2_room(&run.room, &run.inventory);
            let mut rebound = match entry_door.as_deref() {
                Some(door) => Simulation::enter_via_door(room, run.inventory.abilities(), door)
                    .expect("persistent dungeon v2 entry door remains valid"),
                None => Simulation::with_abilities(room, run.inventory.abilities()),
            };
            configure_live_simulation(&mut rebound, self.movement_tuning);
            self.simulation = rebound;
            self.human_recorder = HumanRecorder::new(&self.simulation);
            self.persist_dungeon_v2_progress();
            return true;
        }

        let mut changed = false;
        for id in report.events.iter().filter_map(|event| match event {
            SimulationEvent::PickupCollected { id } => Some(id.as_str()),
            _ => None,
        }) {
            if id.starts_with("v2-coin-") {
                if run.inventory.coins.insert(id.to_owned()) {
                    changed = true;
                    self.replay_notice = Some(ReplayNotice {
                        title: format!(
                            "COIN {}/{}",
                            run.inventory.coins.len(),
                            dungeon_v2_total_coins()
                        ),
                        detail: "Coins persist across rooms and open sealed doors".to_owned(),
                        is_error: false,
                    });
                }
            } else if id == DEMO_DUNGEON_GLOVE_PICKUP && !run.inventory.climbing_gloves {
                run.inventory.climbing_gloves = true;
                changed = true;
                self.simulation.grant_abilities(run.inventory.abilities());
                self.replay_notice = Some(ReplayNotice {
                    title: "CLIMBING GLOVES".to_owned(),
                    detail: "Wall-jumps unlocked · sealed wall doors now open".to_owned(),
                    is_error: false,
                });
            } else if id == DEMO_DUNGEON_BOOT_PICKUP && !run.inventory.winged_boots {
                run.inventory.winged_boots = true;
                changed = true;
                self.simulation.grant_abilities(run.inventory.abilities());
                self.replay_notice = Some(ReplayNotice {
                    title: "WINGED BOOTS".to_owned(),
                    detail: "Dash unlocked · sealed dash doors now open".to_owned(),
                    is_error: false,
                });
            } else if id == DEMO_DUNGEON_CROWN_PICKUP && !run.inventory.crown {
                run.inventory.crown = true;
                changed = true;
                self.replay_notice = Some(ReplayNotice {
                    title: "THE CROWN".to_owned(),
                    detail: "Climb back out · the exit gate at the start is open".to_owned(),
                    is_error: false,
                });
            }
        }
        if changed {
            self.dungeon_v2_run = Some(run.clone());
            self.persist_dungeon_v2_progress();
        }

        let Some(exit_id) = report.events.iter().find_map(|event| match event {
            SimulationEvent::ExitReached { id } => Some(id.as_str()),
            _ => None,
        }) else {
            return changed;
        };
        let Some(door) = self
            .simulation
            .room()
            .doors()
            .iter()
            .find(|door| door.id == exit_id)
            .cloned()
        else {
            // The spawn room's escape gate is a terminal legacy Exit, not a
            // room transition; it only exists in the room once the crown is
            // held, so reaching it ends the run.
            if run.inventory.crown {
                self.dungeon_v2_victory = true;
                self.persist_dungeon_v2_progress();
            }
            return true;
        };
        if let Some(requirement) = dungeon_v2_door_requirement(&run.room, &door.id)
            && !run.inventory.satisfies(requirement)
        {
            let _ = self.simulation.reject_reached_door(&door.id);
            self.human_recorder.resume_at(&self.simulation);
            let label = match requirement {
                DungeonV2Requirement::ClimbingGloves => "CLIMBING GLOVES".to_owned(),
                DungeonV2Requirement::WingedBoots => "WINGED BOOTS".to_owned(),
                DungeonV2Requirement::Coins(count) => format!("{count} COINS"),
            };
            self.replay_notice = Some(ReplayNotice {
                title: format!("SEALED · {label} REQUIRED"),
                detail: format!(
                    "you have {}/{} coins; explore another branch",
                    run.inventory.coins.len(),
                    dungeon_v2_total_coins()
                ),
                is_error: true,
            });
            return true;
        }
        let Some(destination_room) = door.destination_room.clone() else {
            return false;
        };
        let destination_door = door
            .destination_door
            .as_deref()
            .expect("dungeon v2 doors always name their mate");
        run.room = destination_room.clone();
        self.explored_v2_rooms.insert(destination_room.clone());
        let room = dungeon_v2_room(&run.room, &run.inventory);
        let mut simulation =
            Simulation::enter_via_door(room, run.inventory.abilities(), destination_door)
                .expect("dungeon v2 graph points at a validated destination door");
        configure_live_simulation(&mut simulation, self.movement_tuning);
        self.dungeon_v2_run = Some(run.clone());
        self.simulation = simulation;
        self.feedback = SimulationFeedback::default();
        self.human_recorder = HumanRecorder::new(&self.simulation);
        self.replay_mode = ReplayMode::Human;
        self.replay_notice = Some(ReplayNotice {
            title: destination_room.to_ascii_uppercase(),
            detail: format!(
                "coins {}/{}{}",
                run.inventory.coins.len(),
                dungeon_v2_total_coins(),
                if !run.inventory.climbing_gloves {
                    " · find the Climbing Gloves"
                } else if !run.inventory.winged_boots {
                    " · find the Winged Boots"
                } else {
                    " · find the Crown"
                }
            ),
            is_error: false,
        });
        self.persist_dungeon_v2_progress();
        true
    }

    fn observe_floor_lab_progress(&mut self, report: &StepReport) -> bool {
        let RoomMode::Challenge(ChallengeKind::DungeonFloor(_)) = self.selection.mode else {
            return false;
        };
        let Some(exit_id) = report.events.iter().find_map(|event| match event {
            SimulationEvent::ExitReached { id } => Some(id.as_str()),
            _ => None,
        }) else {
            return false;
        };
        let Some(door) = self
            .simulation
            .room()
            .doors()
            .iter()
            .find(|door| door.id == exit_id)
            .cloned()
        else {
            // The crown goal is a terminal legacy Exit, not a room transition.
            return false;
        };
        let Some(destination_room) = door
            .destination_room
            .as_deref()
            .and_then(DemoDungeonRoom::from_id)
        else {
            return false;
        };
        let destination_door = door
            .destination_door
            .as_deref()
            .expect("built-in dungeon doors always name their mate");
        let spec = dungeon_floor_route_spec(destination_room);
        let room = demo_dungeon_room(destination_room, spec.inventory);
        let mut simulation =
            match Simulation::enter_via_door(room, spec.inventory.abilities(), destination_door) {
                Ok(simulation) => simulation,
                Err(error) => {
                    eprintln!(
                        "floor lab cannot enter {} by {destination_door:?}: {error}",
                        destination_room.id()
                    );
                    return false;
                }
            };
        configure_live_simulation(&mut simulation, self.movement_tuning);
        self.selection = ScenarioSelection {
            mode: RoomMode::Challenge(ChallengeKind::DungeonFloor(destination_room)),
            seed: 0,
            tier: AbilityTier::Baseline,
        }
        .canonicalized();
        self.simulation = simulation;
        self.feedback = SimulationFeedback::default();
        self.human_recorder = HumanRecorder::new(&self.simulation);
        self.replay_mode = ReplayMode::Human;
        self.replay_notice = Some(ReplayNotice {
            title: destination_room.title().to_ascii_uppercase(),
            detail: "FLOOR LAB · non-persistent · M browses floors".to_owned(),
            is_error: false,
        });
        true
    }

    fn observe_dungeon_progress(&mut self, report: &StepReport) -> bool {
        let Some(mut run) = self.dungeon_run else {
            return false;
        };

        let reset_needs_inventory_rebind = report
            .events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Reset))
            && self
                .simulation
                .room()
                .pickups()
                .iter()
                .any(|pickup| run.inventory.owns_persistent_pickup(pickup.id()));
        if reset_needs_inventory_rebind {
            let entry_door = self.simulation.entry_door().map(str::to_owned);
            let room = demo_dungeon_room(run.room, run.inventory);
            let mut rebound = match entry_door.as_deref() {
                Some(door) => Simulation::enter_via_door(room, run.inventory.abilities(), door)
                    .expect("persistent dungeon entry door remains valid"),
                None => Simulation::with_abilities(room, run.inventory.abilities()),
            };
            configure_live_simulation(&mut rebound, self.movement_tuning);
            self.simulation = rebound;
            self.human_recorder = HumanRecorder::new(&self.simulation);
            return true;
        }

        let mut collected_coin_ids = Vec::new();
        for id in report.events.iter().filter_map(|event| match event {
            SimulationEvent::PickupCollected { id } => Some(id.as_str()),
            _ => None,
        }) {
            if run.inventory.collect_coin(id) {
                collected_coin_ids.push(id.to_owned());
            }
        }
        if !collected_coin_ids.is_empty() {
            self.dungeon_run = Some(run);
            self.replay_notice = Some(ReplayNotice {
                title: format!(
                    "COIN {}/{}",
                    run.inventory.coin_count(),
                    DEMO_DUNGEON_TOTAL_COINS
                ),
                detail: "Coins persist across rooms and open sealed doors".to_owned(),
                is_error: false,
            });
            self.persist_dungeon_progress();
            for id in &collected_coin_ids {
                self.record_dungeon_event(
                    "coin_collected",
                    run.room,
                    None,
                    None,
                    Some(id),
                    run.inventory,
                );
            }
        }

        let collected_gloves = report.events.iter().any(|event| {
            matches!(event, SimulationEvent::PickupCollected { id } if id == DEMO_DUNGEON_GLOVE_PICKUP)
        });
        if collected_gloves && !run.inventory.climbing_gloves {
            run.inventory.climbing_gloves = true;
            self.dungeon_run = Some(run);
            self.simulation
                .grant_abilities(AbilitySet::new(true, false));
            self.human_recorder.resume_at(&self.simulation);
            self.replay_notice = Some(ReplayNotice {
                title: "CLIMBING GLOVES ACQUIRED".to_owned(),
                detail: "Wall Jump unlocked: hold toward a wall and press Jump".to_owned(),
                is_error: false,
            });
            self.persist_dungeon_progress();
            self.record_dungeon_event(
                "traversal_unlocked",
                run.room,
                None,
                None,
                Some(DEMO_DUNGEON_GLOVE_PICKUP),
                run.inventory,
            );
            return true;
        }

        let collected_boots = report.events.iter().any(|event| {
            matches!(event, SimulationEvent::PickupCollected { id } if id == DEMO_DUNGEON_BOOT_PICKUP)
        });
        if collected_boots && !run.inventory.winged_boots {
            run.inventory.winged_boots = true;
            self.dungeon_run = Some(run);
            self.simulation
                .grant_abilities(AbilitySet::new(false, true));
            self.human_recorder.resume_at(&self.simulation);
            self.replay_notice = Some(ReplayNotice {
                title: "WINGED BOOTS ACQUIRED".to_owned(),
                detail: "Dash unlocked: hold a direction and press X or Shift".to_owned(),
                is_error: false,
            });
            self.persist_dungeon_progress();
            self.record_dungeon_event(
                "traversal_unlocked",
                run.room,
                None,
                None,
                Some(DEMO_DUNGEON_BOOT_PICKUP),
                run.inventory,
            );
            return true;
        }

        if report.events.iter().any(|event| {
            matches!(event, SimulationEvent::PickupCollected { id } if id == DEMO_DUNGEON_CROWN_PICKUP)
        }) {
            run.inventory.crown = true;
            self.dungeon_run = Some(run);
            self.persist_dungeon_progress();
            self.record_dungeon_event(
                "crown_collected",
                run.room,
                None,
                None,
                Some(DEMO_DUNGEON_CROWN_PICKUP),
                run.inventory,
            );
        }

        let Some(exit_id) = report.events.iter().find_map(|event| match event {
            SimulationEvent::ExitReached { id } => Some(id.as_str()),
            _ => None,
        }) else {
            return false;
        };
        let Some(door) = self
            .simulation
            .room()
            .doors()
            .iter()
            .find(|door| door.id == exit_id)
            .cloned()
        else {
            // The crown goal is a terminal legacy Exit, not a room transition.
            return false;
        };
        let requirement = demo_dungeon_door_requirement(run.room, &door.id);
        if !requirement.is_satisfied_by(&run.inventory.authored_progression_inventory()) {
            // The production report and simulation are the same authoritative step. Keeping
            // this tolerant also makes the run-state observer robust to synthetic diagnostics.
            let _ = self.simulation.reject_reached_door(&door.id);
            self.human_recorder.resume_at(&self.simulation);
            self.replay_notice = Some(ReplayNotice {
                title: format!(
                    "SEALED · {} REQUIRED",
                    dungeon_requirement_label(requirement)
                ),
                detail: format!(
                    "you have {}/{} coins; explore another branch",
                    run.inventory.coin_count(),
                    DEMO_DUNGEON_TOTAL_COINS
                ),
                is_error: true,
            });
            self.record_dungeon_event(
                "sealed_door_rejected",
                run.room,
                None,
                Some(&door.id),
                None,
                run.inventory,
            );
            return true;
        }
        let Some(destination_room) = door
            .destination_room
            .as_deref()
            .and_then(DemoDungeonRoom::from_id)
        else {
            self.replay_notice = Some(ReplayNotice {
                title: "BROKEN DUNGEON DOOR".to_owned(),
                detail: format!("{} has no known destination", door.id),
                is_error: true,
            });
            return false;
        };
        let destination_door = door
            .destination_door
            .as_deref()
            .expect("built-in dungeon doors always name their mate");
        let source_room = run.room;
        run.room = destination_room;
        self.explored_rooms.insert(destination_room);
        let room = demo_dungeon_room(run.room, run.inventory);
        let mut simulation =
            Simulation::enter_via_door(room, run.inventory.abilities(), destination_door)
                .expect("built-in dungeon graph points at a validated destination door");
        configure_live_simulation(&mut simulation, self.movement_tuning);
        self.dungeon_run = Some(run);
        self.simulation = simulation;
        self.feedback = SimulationFeedback::default();
        self.human_recorder = HumanRecorder::new(&self.simulation);
        self.replay_mode = ReplayMode::Human;
        self.replay_notice = Some(ReplayNotice {
            title: destination_room.title().to_ascii_uppercase(),
            detail: if !run.inventory.climbing_gloves {
                format!(
                    "Find the Climbing Gloves · coins {}/{}",
                    run.inventory.coin_count(),
                    DEMO_DUNGEON_TOTAL_COINS
                )
            } else if run.inventory.winged_boots {
                format!(
                    "Winged Boots equipped · coins {}/{} · find the Crown",
                    run.inventory.coin_count(),
                    DEMO_DUNGEON_TOTAL_COINS
                )
            } else {
                format!(
                    "Find the Winged Boots · coins {}/{}",
                    run.inventory.coin_count(),
                    DEMO_DUNGEON_TOTAL_COINS
                )
            },
            is_error: false,
        });
        self.persist_dungeon_progress();
        self.record_dungeon_event(
            "room_transition",
            source_room,
            Some(destination_room),
            Some(&door.id),
            None,
            run.inventory,
        );
        true
    }
}

// Embedding makes the verified offline selection part of the executable and avoids depending on
// the process working directory at play time.
const CATALOGUE_SOURCES: [(&str, &str); 4] = [
    (
        "content/catalogues/v6/baseline.manifest",
        include_str!("../../../content/catalogues/v6/baseline.manifest"),
    ),
    (
        "content/catalogues/v6/wall.manifest",
        include_str!("../../../content/catalogues/v6/wall.manifest"),
    ),
    (
        "content/catalogues/v6/dash.manifest",
        include_str!("../../../content/catalogues/v6/dash.manifest"),
    ),
    (
        "content/catalogues/v6/both.manifest",
        include_str!("../../../content/catalogues/v6/both.manifest"),
    ),
];

fn load_playable_catalogue(
    corpus_manifest: Option<&str>,
    allow_provisional_corpus: bool,
) -> Result<PlayableCatalogue, String> {
    match corpus_manifest {
        Some(path) => {
            let source = fs::read_to_string(path)
                .map_err(|error| format!("could not read corpus manifest {path:?}: {error}"))?;
            PlayableCatalogue::parse_corpus(&source, allow_provisional_corpus)
        }
        None => PlayableCatalogue::parse(&CATALOGUE_SOURCES),
    }
}

fn load_scenario(
    selection: ScenarioSelection,
    catalogue: &PlayableCatalogue,
) -> Result<(Simulation, Option<GeneratedProvenance>), String> {
    let (simulation, generated_provenance) = match selection.mode {
        RoomMode::Generated => {
            let entry = catalogue
                .entry(selection.tier, selection.seed as usize)
                .ok_or_else(|| {
                    format!(
                        "curated entry {} does not exist for {}",
                        selection.seed,
                        tier_label(selection.tier)
                    )
                })?;
            if let Some(simulation) = entry.load_corpus()? {
                let corpus = entry.corpus().expect("corpus loader matched corpus entry");
                let provenance = GeneratedProvenance {
                    generation_version: 0,
                    seed: corpus.key().source_seed(),
                    strategy: GenerationStrategy::CyclicGraph,
                    intent: downwards_gen::experimental::ChallengeIntent::Standard,
                    corpus_generator: Some(corpus.key().generator_slug()),
                };
                (simulation, Some(provenance))
            } else {
                let legacy = entry.legacy().expect("non-corpus entry is legacy");
                let key = legacy.key();
                let candidate = generate_compositional(key).map_err(|error| {
                    format!("catalogue room {:?} failed generation: {error}", entry.id())
                })?;
                let provenance = GeneratedProvenance {
                    generation_version: COMPOSITIONAL_GENERATION_VERSION,
                    seed: key.seed,
                    strategy: key.profile.strategy,
                    intent: key.profile.intent,
                    corpus_generator: None,
                };
                let simulation = Simulation::enter_via_door(
                    candidate.generated.room,
                    key.profile.abilities,
                    entry.source_door_id(),
                )
                .map_err(|error| {
                    format!(
                        "catalogue room {:?} has invalid source: {error}",
                        entry.id()
                    )
                })?;
                (simulation, Some(provenance))
            }
        }
        RoomMode::DeveloperGenerated => {
            let candidate = generate_uncurated(selection.seed, selection.tier.abilities())
                .map_err(|error| {
                    format!(
                        "developer seed {} failed generation: {error}",
                        selection.seed
                    )
                })?;
            let key = candidate.key;
            let provenance = GeneratedProvenance {
                generation_version: COMPOSITIONAL_GENERATION_VERSION,
                seed: key.seed,
                strategy: key.profile.strategy,
                intent: key.profile.intent,
                corpus_generator: None,
            };
            (
                Simulation::with_abilities(candidate.generated.room, key.profile.abilities),
                Some(provenance),
            )
        }
        RoomMode::Development => {
            let room = first_steps_room();
            (
                Simulation::with_abilities(room, selection.tier.abilities()),
                None,
            )
        }
        RoomMode::Gallery => {
            let index = usize::try_from(selection.seed).map_err(|_| {
                format!(
                    "gallery index {} does not fit this platform",
                    selection.seed
                )
            })?;
            let level = calibration_gallery().get(index).copied().ok_or_else(|| {
                format!(
                    "gallery entry {} does not exist ({} registered)",
                    selection.seed,
                    calibration_gallery().len()
                )
            })?;
            if level.abilities().dash {
                return Err(format!(
                    "gallery entry {:?} violates the no-Dash gallery contract",
                    level.id()
                ));
            }
            let simulation = level.scenario();
            if simulation.abilities() != level.abilities() {
                return Err(format!(
                    "gallery entry {:?} scenario loadout does not match its content metadata",
                    level.id()
                ));
            }
            if !simulation
                .room()
                .exits()
                .iter()
                .any(|exit| exit.id == level.target())
            {
                return Err(format!(
                    "gallery entry {:?} is missing target exit {:?}",
                    level.id(),
                    level.target()
                ));
            }
            (simulation, None)
        }
        RoomMode::CalibratedGenerated => {
            let index = usize::try_from(selection.seed).map_err(|_| {
                format!(
                    "calibrated generated index {} does not fit this platform",
                    selection.seed
                )
            })?;
            let level = calibrated_generator_playtest()
                .get(index)
                .copied()
                .ok_or_else(|| {
                    format!(
                        "calibrated generated entry {} does not exist ({} registered)",
                        selection.seed,
                        calibrated_generator_playtest().len()
                    )
                })?;
            (level.scenario(), None)
        }
        RoomMode::Dungeon => {
            let run = DungeonRunState::default();
            (
                Simulation::with_abilities(
                    demo_dungeon_room(run.room, run.inventory),
                    run.inventory.abilities(),
                ),
                None,
            )
        }
        RoomMode::DungeonV2 => {
            let run = DungeonV2RunState::default();
            (
                Simulation::with_abilities(
                    dungeon_v2_room(&run.room, &run.inventory),
                    run.inventory.abilities(),
                ),
                None,
            )
        }
        RoomMode::Challenge(kind) => {
            let simulation = kind.scenario();
            let expected_abilities = kind.abilities();
            debug_assert_eq!(simulation.abilities(), expected_abilities);
            (simulation, None)
        }
    };
    Ok((simulation, generated_provenance))
}

fn is_movement_course_room(room: &Room) -> bool {
    room.id().starts_with("movement.course.")
}

fn configure_live_simulation(simulation: &mut Simulation, movement_tuning: MovementTuning) {
    simulation.enable_human_wall_assists();
    simulation.set_movement_tuning(movement_tuning);
}

fn adjust_tuning_value(value: u16, delta: i16, minimum: u16, maximum: u16) -> u16 {
    let adjusted = i32::from(value) + i32::from(delta);
    adjusted.clamp(i32::from(minimum), i32::from(maximum)) as u16
}

fn movement_tuning_summary(tuning: MovementTuning) -> String {
    format!(
        "{} px/s | accel {} ms | brake {} ms | wall ascent {}% | wall-jump boost {}% / {} ms",
        tuning.top_speed_pixels_per_second,
        tuning.acceleration_milliseconds,
        tuning.braking_milliseconds,
        tuning.wall_ascent_carry_percent,
        tuning.wall_carry_percent,
        tuning.wall_momentum_milliseconds,
    )
}

fn death_reason_label(reason: DeathReason) -> &'static str {
    match reason {
        DeathReason::Hazard { .. } => "SPIKES",
        DeathReason::TimedHazard { .. } => "TIMED TRAP",
    }
}

fn jump_kind_label(kind: downwards_core::JumpKind) -> String {
    match kind {
        downwards_core::JumpKind::Grounded => "grounded".to_owned(),
        downwards_core::JumpKind::Coyote => "coyote".to_owned(),
        downwards_core::JumpKind::Buffered => "buffered_landing".to_owned(),
        downwards_core::JumpKind::Wall {
            side: WallSide::Left,
        } => "wall_left".to_owned(),
        downwards_core::JumpKind::Wall {
            side: WallSide::Right,
        } => "wall_right".to_owned(),
    }
}

fn attempt_outcome(
    events: &[SimulationEvent],
    expected_target_door: Option<&str>,
) -> Option<AttemptOutcome> {
    if let Some(reason) = events.iter().find_map(|event| match event {
        SimulationEvent::Died(reason) => Some(*reason),
        _ => None,
    }) {
        return Some(AttemptOutcome::Died(reason));
    }
    if let Some(id) = events.iter().find_map(|event| match event {
        SimulationEvent::ExitReached { id } => Some(id.clone()),
        _ => None,
    }) {
        return Some(if expected_target_door.is_none_or(|target| target == id) {
            AttemptOutcome::Exit(id)
        } else {
            AttemptOutcome::WrongDoor(id)
        });
    }
    events
        .iter()
        .any(|event| matches!(event, SimulationEvent::Reset))
        .then_some(AttemptOutcome::Reset)
}

fn inconclusive_reason_label(reason: InconclusiveReason) -> &'static str {
    match reason {
        InconclusiveReason::NoExitsDefined => "NO EXITS",
        InconclusiveReason::ExpandedNodeBudget => "NODE BUDGET",
        InconclusiveReason::SimulatedTickBudget => "TICK BUDGET",
        InconclusiveReason::PathHorizon => "PATH HORIZON",
        InconclusiveReason::FrontierExhausted => "FRONTIER EXHAUSTED",
    }
}

fn tier_number(tier: AbilityTier) -> u8 {
    match tier {
        AbilityTier::Baseline => 1,
        AbilityTier::WallJump => 2,
        AbilityTier::Dash => 3,
        AbilityTier::WallJumpAndDash => 4,
    }
}

fn previous_ability_tier(tier: AbilityTier) -> AbilityTier {
    match tier {
        AbilityTier::Baseline => AbilityTier::WallJumpAndDash,
        AbilityTier::WallJump => AbilityTier::Baseline,
        AbilityTier::Dash => AbilityTier::WallJump,
        AbilityTier::WallJumpAndDash => AbilityTier::Dash,
    }
}

fn next_ability_tier(tier: AbilityTier) -> AbilityTier {
    match tier {
        AbilityTier::Baseline => AbilityTier::WallJump,
        AbilityTier::WallJump => AbilityTier::Dash,
        AbilityTier::Dash => AbilityTier::WallJumpAndDash,
        AbilityTier::WallJumpAndDash => AbilityTier::Baseline,
    }
}

/// Converts render-frame keyboard state into a deterministic fixed-step jump signal.
///
/// Press and release edges are retained in order until fixed simulation ticks consume them. Core
/// physics, rather than this adapter, maps an ordinary tap envelope to one fixed low arc and a
/// continued hold to additional height.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct HumanJumpInput {
    applied_held: bool,
    transitions: VecDeque<bool>,
    physical_press_started_micros: Option<u64>,
    held_gesture_committed: bool,
    tap_waiting_for_acceptance: bool,
    tap_signal_ticks: u8,
}

impl HumanJumpInput {
    fn observe_frame(
        &mut self,
        pressed: bool,
        released: bool,
        held: bool,
        observed_micros: u64,
        immediate_wall_jump: bool,
    ) {
        // Macroquad can retain both edges when a tap occurs inside one render frame. When the
        // aggregate state finishes released, preserve that tap as true then false. When it
        // finishes held, another jump alias may have remained down throughout; suppressing an
        // unobservable sub-tick false edge avoids cutting that continuously held jump. A release
        // and re-press observed on distinct frames are still retained as two ordered transitions.
        if pressed && self.physical_press_started_micros.is_none() {
            self.physical_press_started_micros = Some(observed_micros);
        }
        if pressed && immediate_wall_jump {
            self.held_gesture_committed = true;
            self.queue_transition(true);
        }
        if pressed && released && !held {
            if immediate_wall_jump {
                self.queue_transition(false);
                self.held_gesture_committed = false;
            } else {
                self.commit_tap();
            }
            self.physical_press_started_micros = None;
            return;
        }

        if held
            && !self.held_gesture_committed
            && self.physical_press_started_micros.is_some_and(|started| {
                observed_micros.saturating_sub(started) >= HUMAN_JUMP_TAP_WINDOW_MICROS
            })
        {
            self.held_gesture_committed = true;
            self.queue_transition(true);
        }

        if released && !held {
            if self.held_gesture_committed {
                self.queue_transition(false);
            } else {
                self.commit_tap();
            }
            self.physical_press_started_micros = None;
            self.held_gesture_committed = false;
        }
    }

    fn action_for_tick(&mut self) -> bool {
        if let Some(next) = self.transitions.pop_front() {
            self.applied_held = next;
        }
        if self.tap_waiting_for_acceptance && self.applied_held {
            self.tap_signal_ticks = self.tap_signal_ticks.saturating_add(1);
        }
        self.applied_held
    }

    fn observe_simulation_step(&mut self, events: &[SimulationEvent]) {
        if events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Reset))
        {
            self.reset();
            return;
        }
        let accepted = events
            .iter()
            .any(|event| matches!(event, SimulationEvent::Jumped(_)));
        if self.tap_waiting_for_acceptance
            && (accepted || self.tap_signal_ticks >= JUMP_BUFFER_TICKS)
        {
            self.tap_waiting_for_acceptance = false;
            self.tap_signal_ticks = 0;
            self.queue_transition(false);
        }
    }

    fn reset(&mut self) {
        *self = Self::default();
    }

    fn queue_transition(&mut self, held: bool) {
        let previous = self
            .transitions
            .back()
            .copied()
            .unwrap_or(self.applied_held);
        if previous != held {
            self.transitions.push_back(held);
        }
    }

    fn commit_tap(&mut self) {
        if self.held_gesture_committed || self.tap_waiting_for_acceptance {
            return;
        }
        self.tap_waiting_for_acceptance = true;
        self.tap_signal_ticks = 0;
        self.queue_transition(true);
    }
}

fn room_mode_label(mode: RoomMode) -> &'static str {
    match mode {
        RoomMode::Generated => "CURATED",
        RoomMode::DeveloperGenerated => "DEVELOPER",
        RoomMode::Development => "DEVELOPMENT",
        RoomMode::Gallery => "GALLERY",
        RoomMode::CalibratedGenerated => "CALIBRATED",
        RoomMode::Dungeon => "DUNGEON",
        RoomMode::DungeonV2 => "DUNGEON V2",
        RoomMode::Challenge(ChallengeKind::DungeonFloor(_)) => "DUNGEON FLOOR LAB",
        RoomMode::Challenge(ChallengeKind::Hard | ChallengeKind::Medium) => "CHALLENGE",
    }
}

fn tier_label(tier: AbilityTier) -> &'static str {
    match tier {
        AbilityTier::Baseline => "BASELINE",
        AbilityTier::WallJump => "WALL JUMP",
        AbilityTier::Dash => "DASH",
        AbilityTier::WallJumpAndDash => "WALL + DASH",
    }
}

fn tier_for_abilities(abilities: AbilitySet) -> AbilityTier {
    match (abilities.wall_jump, abilities.dash) {
        (false, false) => AbilityTier::Baseline,
        (true, false) => AbilityTier::WallJump,
        (false, true) => AbilityTier::Dash,
        (true, true) => AbilityTier::WallJumpAndDash,
    }
}

fn kit_tab_label(tier: AbilityTier) -> &'static str {
    match tier {
        AbilityTier::Baseline => "1 BASIC",
        AbilityTier::WallJump => "2 WALL JUMP",
        AbilityTier::Dash => "3 DASH",
        AbilityTier::WallJumpAndDash => "4 BOTH",
    }
}

fn catalogue_band_label(band: CatalogueBand) -> &'static str {
    match band {
        CatalogueBand::Gentle => "GENTLE",
        CatalogueBand::Standard => "STANDARD",
        CatalogueBand::Technical => "TECHNICAL",
    }
}

fn compact_band_label(band: CatalogueBand) -> &'static str {
    match band {
        CatalogueBand::Gentle => "G",
        CatalogueBand::Standard => "S",
        CatalogueBand::Technical => "T",
    }
}

fn band_colour(band: CatalogueBand) -> Color {
    match band {
        CatalogueBand::Gentle => EXIT,
        CatalogueBand::Standard => PLAYER_ACCENT,
        CatalogueBand::Technical => HAZARD,
    }
}

fn compact_strategy_label(strategy: GenerationStrategy) -> &'static str {
    match strategy {
        GenerationStrategy::CyclicGraph => "CG",
        GenerationStrategy::ReachabilityGrowth => "RG",
        GenerationStrategy::RhythmWeave => "RW",
    }
}

const DEVELOPMENT_LEVEL_IDENTIFIER: &str = "FIRST STEPS";
const HARD_NO_DASH_LEVEL_IDENTIFIER: &str = "HARD NO-DASH CHALLENGE";
const MEDIUM_NO_DASH_LEVEL_IDENTIFIER: &str = "MEDIUM NO-DASH CHALLENGE";

fn compact_ability_summary(tier: AbilityTier) -> &'static str {
    match tier {
        AbilityTier::Baseline => "WALL:LOCKED  DASH:LOCKED",
        AbilityTier::WallJump => "WALL:ON (HOLD INTO + JUMP)  DASH:LOCKED",
        AbilityTier::Dash => "WALL:LOCKED  DASH:X/SHIFT+DIR",
        AbilityTier::WallJumpAndDash => "WALL:ON (HOLD INTO + JUMP)  DASH:X/SHIFT+DIR",
    }
}

fn gameplay_control_summary(tier: AbilityTier) -> &'static str {
    match tier {
        AbilityTier::Baseline => {
            "A/D MOVE  JUMP SPACE/Z: TAP=LOW HOLD=HIGH  R RETRY  DASH LOCKED  M"
        }
        AbilityTier::WallJump => "A/D MOVE  JUMP: TAP=LOW HOLD=HIGH  WALL: HOLD TOWARD + JUMP  R/M",
        AbilityTier::Dash => "A/D MOVE  JUMP: TAP=LOW HOLD=HIGH  DASH X/SHIFT+DIR  LAND REFILLS  M",
        AbilityTier::WallJumpAndDash => {
            "A/D MOVE  JUMP: TAP=LOW HOLD=HIGH  WALL: HOLD TOWARD + JUMP  DASH X  M"
        }
    }
}

fn verified_route_mechanics(tier: AbilityTier, verified_counts: Option<(usize, usize)>) -> String {
    let abilities = tier.abilities();
    let wall = if abilities.wall_jump { "ON" } else { "LOCKED" };
    let dash = if abilities.dash { "ON" } else { "LOCKED" };
    verified_counts.map_or_else(
        || format!("KIT WALL:{wall} DASH:{dash} | NO VERIFIED ROUTE"),
        |(wall_jumps, dashes)| {
            format!("KIT WALL:{wall} DASH:{dash} | ROUTE WJ {wall_jumps} DASH {dashes}")
        },
    )
}

fn horizontal_input() -> i8 {
    let left = is_key_down(KeyCode::Left) || is_key_down(KeyCode::A);
    let right = is_key_down(KeyCode::Right) || is_key_down(KeyCode::D);
    i8::from(right) - i8::from(left)
}

fn menu_navigation_held() -> bool {
    [
        KeyCode::Left,
        KeyCode::Right,
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::A,
        KeyCode::D,
        KeyCode::W,
        KeyCode::S,
        KeyCode::LeftBracket,
        KeyCode::RightBracket,
    ]
    .into_iter()
    .any(is_key_down)
}

fn vertical_input() -> i8 {
    let up = is_key_down(KeyCode::Up) || is_key_down(KeyCode::W);
    let down = is_key_down(KeyCode::Down) || is_key_down(KeyCode::S);
    i8::from(down) - i8::from(up)
}

fn sample_raw_jump_input(render_frame: u64, sampled_at_session_us: u64) -> RawJumpInputSample {
    let sample = |key, code| RawJumpKeySample {
        key,
        pressed: is_key_pressed(code),
        released: is_key_released(code),
        held: is_key_down(code),
    };
    RawJumpInputSample {
        render_frame,
        sampled_at_session_us,
        keys: [
            sample(HumanJumpKey::Space, KeyCode::Space),
            sample(HumanJumpKey::Z, KeyCode::Z),
            sample(HumanJumpKey::Up, KeyCode::Up),
        ],
    }
}

fn dash_held() -> bool {
    is_key_down(KeyCode::X) || is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift)
}

fn dash_pressed() -> bool {
    is_key_pressed(KeyCode::X)
        || is_key_pressed(KeyCode::LeftShift)
        || is_key_pressed(KeyCode::RightShift)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnvironmentSprite {
    SolidA = 0,
    SolidB = 1,
    OneWay = 2,
    SpikesUp = 3,
    SpikesDown = 4,
    SpikesHorizontal = 5,
    Exit = 7,
    Door = 8,
    Pickup = 9,
}

impl EnvironmentSprite {
    const fn sheet_source(self) -> Rect {
        let index = self as u8;
        let column = index % ENVIRONMENT_SHEET_COLUMNS;
        let row = index / ENVIRONMENT_SHEET_COLUMNS;
        debug_assert!(row < ENVIRONMENT_SHEET_ROWS);
        Rect::new(
            column as f32 * ENVIRONMENT_CELL_PIXELS,
            row as f32 * ENVIRONMENT_CELL_PIXELS,
            ENVIRONMENT_CELL_PIXELS,
            ENVIRONMENT_CELL_PIXELS,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlayerPose {
    IdleA = 0,
    IdleB = 1,
    RunContact = 2,
    RunPassing = 3,
    RunContactOpposite = 4,
    RunPassingOpposite = 5,
    Rising = 6,
    Falling = 7,
    WallCling = 8,
    WallJump = 9,
    Skid = 10,
    DeepSkid = 11,
}

impl PlayerPose {
    const fn sheet_source(self) -> Rect {
        let index = self as u8;
        let column = index % PLAYER_SPRITE_SHEET_COLUMNS;
        let row = index / PLAYER_SPRITE_SHEET_COLUMNS;
        debug_assert!(row < PLAYER_SPRITE_SHEET_ROWS);
        Rect::new(
            column as f32 * PLAYER_SPRITE_CELL_PIXELS,
            row as f32 * PLAYER_SPRITE_CELL_PIXELS,
            PLAYER_SPRITE_CELL_PIXELS,
            PLAYER_SPRITE_CELL_PIXELS,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlayerVisual {
    pose: PlayerPose,
    flip_x: bool,
}

fn select_player_visual(client: &ClientState) -> PlayerVisual {
    let simulation = &client.simulation;
    let player = simulation.player();
    let facing_left = player.facing() < 0;
    if player.dash_compressed() {
        return PlayerVisual {
            // The deep-skid cell is the existing clearly tucked silhouette. Keeping the feet
            // anchored to the compressed collider makes the squeeze readable without inventing
            // a second invisible body shape.
            pose: PlayerPose::DeepSkid,
            flip_x: facing_left,
        };
    }
    if client.feedback.wall_jump_ticks > 0 {
        // The authored wall-jump cell departs left from a right-hand wall.
        return PlayerVisual {
            pose: PlayerPose::WallJump,
            flip_x: client.feedback.wall_jump_side == Some(WallSide::Left),
        };
    }
    if client.feedback.skid_ticks > 0 {
        return PlayerVisual {
            pose: if client.feedback.skid_ticks > SKID_FEEDBACK_TICKS / 2 {
                PlayerPose::DeepSkid
            } else {
                PlayerPose::Skid
            },
            flip_x: facing_left,
        };
    }
    if let Some(side) = wall_slide_cue(
        simulation.abilities().wall_jump,
        player.wall_sliding(),
        player.wall_contact(),
    ) {
        return PlayerVisual {
            pose: PlayerPose::WallCling,
            flip_x: side == WallSide::Left,
        };
    }
    let velocity = player.velocity_subpixels();
    if !player.grounded() {
        return PlayerVisual {
            pose: if velocity.y < 0 {
                PlayerPose::Rising
            } else {
                PlayerPose::Falling
            },
            flip_x: facing_left,
        };
    }
    if velocity.x.abs() >= SUBPIXELS_PER_PIXEL / 2 {
        let poses = [
            PlayerPose::RunContact,
            PlayerPose::RunPassing,
            PlayerPose::RunContactOpposite,
            PlayerPose::RunPassingOpposite,
        ];
        return PlayerVisual {
            pose: poses[(simulation.room_tick() as usize / 3) % poses.len()],
            flip_x: facing_left,
        };
    }
    PlayerVisual {
        pose: if (simulation.room_tick() / 30).is_multiple_of(2) {
            PlayerPose::IdleA
        } else {
            PlayerPose::IdleB
        },
        flip_x: facing_left,
    }
}

fn render(client: &ClientState, visual_assets: &VisualAssets) {
    clear_background(LETTERBOX);
    let viewport = PixelViewport::for_window(screen_width(), screen_height());
    viewport.fill_logical_screen(ROOM_BACKGROUND);

    if client.level_menu_visible() {
        let menu_viewport = viewport.translated(0, ROOM_TOP);
        if client.gallery_menu_visible() {
            if client.browser_mode == BrowserMode::DungeonFloors {
                draw_dungeon_floor_menu(&menu_viewport, client, visual_assets);
            } else {
                draw_gallery_menu(&menu_viewport, client, visual_assets);
            }
        } else {
            draw_level_menu(&menu_viewport, client, visual_assets);
        }
        return;
    }

    if client.dungeon_v2_victory {
        draw_dungeon_v2_victory(&viewport.translated(0, ROOM_TOP), client);
        return;
    }
    if client.pause_visible {
        let menu_viewport = viewport.translated(0, ROOM_TOP);
        if client.controls_visible {
            draw_controls_screen(&menu_viewport);
        } else {
            draw_dungeon_v2_pause(&menu_viewport, client);
        }
        return;
    }
    if client.map_visible {
        let map_viewport = viewport.translated(0, ROOM_TOP);
        if client.selection.mode == RoomMode::DungeonV2 {
            draw_dungeon_v2_map(&map_viewport, client);
        } else {
            draw_dungeon_map(&map_viewport, client);
        }
        return;
    }

    let room_viewport = viewport.translated(0, ROOM_TOP);
    draw_tiles(&room_viewport, client.simulation.room(), visual_assets);
    draw_room_objects(&room_viewport, &client.simulation, visual_assets);
    let focus = client.current_entry().map(|entry| DoorFocus {
        source: entry.source_door_id(),
        target: entry.target_door_id(),
    });
    draw_exits(
        &room_viewport,
        client.simulation.room(),
        focus,
        visual_assets,
    );
    if client.selection.mode == RoomMode::DungeonV2 {
        draw_dungeon_v2_exit_gate(&room_viewport, client);
    }
    draw_dungeon_coin_gates(&room_viewport, client);
    draw_player_effects(
        &room_viewport,
        client.simulation.player().bounds(),
        wall_slide_cue(
            client.simulation.abilities().wall_jump,
            client.simulation.player().wall_sliding(),
            client.simulation.player().wall_contact(),
        ),
        &client.feedback,
        client.simulation.room_tick(),
    );
    draw_animated_player(
        &room_viewport,
        client.simulation.player().bounds(),
        select_player_visual(client),
        visual_assets,
    );
    if client.debug_visible {
        draw_debug_outlines(&room_viewport, &client.simulation);
    }
    draw_hud(&viewport, client);

    if client.feedback.death_ticks > 0 {
        draw_death_feedback(&room_viewport, client);
    }
    if client.selected_route_complete() {
        draw_win_feedback(&room_viewport, client);
    } else if client.wrong_door().is_some() {
        draw_wrong_door_feedback(&room_viewport, client);
    }
    draw_replay_status(&viewport, &room_viewport, client);
    if client.movement_tuning_menu_visible() {
        draw_movement_tuning_menu(&room_viewport, client);
    }
    if client.debug_visible {
        draw_debug_overlay(&room_viewport, client);
    }
}

fn draw_gallery_menu(viewport: &PixelViewport, client: &ClientState, visual_assets: &VisualAssets) {
    const PREVIEW_X: i32 = 171;
    const PREVIEW_Y: i32 = 43;
    const PREVIEW_SCALE: f32 = 0.43;

    viewport.rectangle(CoreRect::new(0, 0, 320, 180), MENU_BACKGROUND);
    viewport.centered_text("DOWNWARDS  /  CALIBRATION GALLERY", 12, 9, PLAYER);
    viewport.centered_text(
        &format!(
            "{} AUTHORED ROOMS / MECHANICS ONLY / NOT DIFFICULTY-RANKED",
            client.gallery_menu.item_count
        ),
        24,
        5,
        PLAYER_ACCENT,
    );

    let visible = client.gallery_menu.visible_indices();
    let visible_end = visible.end;
    for (row, index) in visible.enumerate() {
        let y = 42 + row as i32 * 11;
        let Some(level) = calibration_gallery().get(index).copied() else {
            continue;
        };
        if index == client.gallery_menu.selected_index {
            viewport.rectangle(CoreRect::new(4, y - 8, 159, 10), MENU_SELECTED);
            viewport.rectangle_outline(CoreRect::new(4, y - 8, 159, 10), 1, PLAYER_ACCENT);
            viewport.text(">", 6, y, 6, PLAYER_ACCENT);
        }
        viewport.text(&level.id().to_ascii_uppercase(), 13, y, 5, UI_DIM);
        viewport.text(level.title(), 50, y, 6, UI_TEXT);
    }
    if client.gallery_menu.first_visible_index > 0 {
        viewport.text("^", 164, 42, 6, PLAYER_ACCENT);
    }
    if visible_end < client.gallery_menu.item_count {
        viewport.text("v", 164, 141, 6, PLAYER_ACCENT);
    }

    viewport.rectangle(CoreRect::new(168, 38, 146, 84), DEBUG_PANEL);
    viewport.rectangle_outline(CoreRect::new(168, 38, 146, 84), 1, UI_DIM);
    if let Some(simulation) = &client.level_menu_preview.simulation {
        viewport.rectangle(
            CoreRect::new(PREVIEW_X, PREVIEW_Y, 138, 78),
            ROOM_BACKGROUND,
        );
        let preview_viewport = PixelViewport {
            scale: viewport.scale * PREVIEW_SCALE,
            left: viewport.screen_x(PREVIEW_X),
            top: viewport.screen_y(PREVIEW_Y),
        };
        draw_tiles(&preview_viewport, simulation.room(), visual_assets);
        draw_room_objects(&preview_viewport, simulation, visual_assets);
        draw_exits(&preview_viewport, simulation.room(), None, visual_assets);
        draw_animated_player(
            &preview_viewport,
            simulation.player().bounds(),
            PlayerVisual {
                pose: PlayerPose::IdleA,
                flip_x: false,
            },
            visual_assets,
        );
    } else {
        viewport.centered_text_in(
            CoreRect::new(168, 38, 146, 84),
            "PREVIEW UNAVAILABLE",
            78,
            7,
            HAZARD,
        );
    }

    if let Some(selection) = client.gallery_menu.selected_scenario()
        && let Some(level) = calibration_gallery()
            .get(client.gallery_menu.selected_index)
            .copied()
    {
        let stats = client.stats_for(selection);
        viewport.text(
            &fit_preview_line(&format!(
                "{} / {}",
                level.id().to_ascii_uppercase(),
                level.title().to_ascii_uppercase()
            )),
            170,
            132,
            5,
            PLAYER,
        );
        viewport.text(
            &fit_preview_line(&level.mechanic_axis().to_ascii_uppercase()),
            170,
            139,
            5,
            WALL_SLIDE_CUE,
        );
        viewport.text(
            &fit_preview_line(&format!(
                "WALL:{} DASH:LOCKED / V TRACTABILITY WITNESS",
                if level.abilities().wall_jump {
                    "ON"
                } else {
                    "LOCKED"
                }
            )),
            170,
            146,
            5,
            PLAYER_ACCENT,
        );
        viewport.text(
            &fit_preview_line(&format!(
                "RUNS {} D {} C {} BEST {}",
                stats.attempts,
                stats.deaths,
                stats.clears,
                format_best_clear(stats.best_clear_ticks)
            )),
            170,
            153,
            5,
            UI_TEXT,
        );
    } else {
        viewport.centered_text("NO GALLERY LEVELS REGISTERED", 137, 7, HAZARD);
    }
    if let Some(error) = &client.level_menu_preview.error {
        viewport.text(&fit_preview_line(error), 170, 153, 5, HAZARD);
    }

    viewport.centered_text(
        "ARROWS/WASD PREVIOUS/NEXT   ENTER PLAY   V WITNESS",
        164,
        6,
        UI_TEXT,
    );
    let scroll_hint = if visible_end < client.gallery_menu.item_count {
        "   v MORE BELOW"
    } else if client.gallery_menu.first_visible_index > 0 {
        "   ^ MORE ABOVE"
    } else {
        ""
    };
    let selected_position = if client.gallery_menu.item_count == 0 {
        0
    } else {
        client.gallery_menu.selected_index.saturating_add(1)
    };
    viewport.centered_text(
        &format!(
            "LEVEL {}/{}   HOME/END FIRST/LAST{scroll_hint}",
            selected_position, client.gallery_menu.item_count
        ),
        174,
        6,
        UI_DIM,
    );
}

fn draw_dungeon_v2_pause(viewport: &PixelViewport, client: &ClientState) {
    viewport.rectangle(CoreRect::new(0, 0, 320, 180), MENU_BACKGROUND);
    viewport.centered_text("PAUSED", 14, 9, PLAYER);
    let Some(run) = client.dungeon_v2_run.as_ref() else {
        return;
    };
    let dungeon = dungeon_v2_definition();
    let seconds = run.play_ticks / 60;
    let sealed_seen = client
        .explored_v2_rooms
        .iter()
        .flat_map(|instance_id| {
            dungeon.instances[instance_id]
                .connections
                .keys()
                .map(move |door_id| (instance_id, door_id))
        })
        .filter(|(instance_id, door_id)| {
            dungeon_v2_door_requirement(instance_id, door_id)
                .is_some_and(|requirement| !run.inventory.satisfies(requirement))
        })
        .count();
    let lines = [
        format!("ROOM        {}", run.room.to_ascii_uppercase()),
        format!(
            "EXPLORED    {}/{} ROOMS",
            client.explored_v2_rooms.len(),
            dungeon.instances.len()
        ),
        format!(
            "COINS       {}/{}",
            run.inventory.coins.len(),
            dungeon_v2_total_coins()
        ),
        format!(
            "GEAR        GLOVES {} · BOOTS {} · CROWN {}",
            if run.inventory.climbing_gloves {
                "YES"
            } else {
                "NO"
            },
            if run.inventory.winged_boots {
                "YES"
            } else {
                "NO"
            },
            if run.inventory.crown { "YES" } else { "NO" },
        ),
        format!("SEALED DOORS SEEN  {sealed_seen}"),
        format!("DEATHS      {}", run.deaths),
        format!(
            "TIME        {}:{:02}:{:02}",
            seconds / 3600,
            (seconds / 60) % 60,
            seconds % 60
        ),
    ];
    for (index, line) in lines.iter().enumerate() {
        viewport.text(line, 70, 33 + index as i32 * 11, 6, UI_TEXT);
    }
    for (index, item) in PAUSE_MENU_ITEMS.iter().enumerate() {
        let selected = index == client.pause_menu_index;
        let y = 118 + index as i32 * 12;
        if selected {
            viewport.rectangle(CoreRect::new(110, y - 8, 100, 10), MENU_SELECTED);
            viewport.rectangle_outline(CoreRect::new(110, y - 8, 100, 10), 1, PLAYER_ACCENT);
        }
        viewport.centered_text(item, y, 6, if selected { PLAYER } else { UI_DIM });
    }
    viewport.centered_text(
        "UP/DOWN SELECT · ENTER CONFIRM · ESC RESUME · TAB MAP",
        172,
        5,
        UI_DIM,
    );
}

/// The escape gate marking in the dungeon v2 spawn room. Drawn in every
/// crown state so the player learns where the run will end: dim and inert
/// before the crown, bright once the crown is held and the exit trigger
/// exists.
fn draw_dungeon_v2_exit_gate(viewport: &PixelViewport, client: &ClientState) {
    let Some(run) = client.dungeon_v2_run.as_ref() else {
        return;
    };
    if run.room != dungeon_v2_definition().spawn {
        return;
    }
    let bounds = dungeon_v2_exit_gate_bounds();
    let crowned = run.inventory.crown;
    let accent = if crowned { EXIT } else { UI_DIM };
    viewport.rectangle(bounds, if crowned { EXIT_DARK } else { DEBUG_PANEL });
    viewport.rectangle_outline(bounds, 1, accent);
    // Chevron pointing out through the west wall.
    let centre_y = bounds.y + bounds.height / 2;
    let arrow_x = bounds.x + bounds.width - 2;
    viewport.triangle(
        (arrow_x, centre_y - 3),
        (arrow_x - 3, centre_y),
        (arrow_x, centre_y + 3),
        accent,
    );
    viewport.text("EXIT", bounds.x + bounds.width + 3, centre_y + 2, 5, accent);
}

fn draw_dungeon_v2_victory(viewport: &PixelViewport, client: &ClientState) {
    viewport.rectangle(CoreRect::new(0, 0, 320, 180), MENU_BACKGROUND);
    viewport.centered_text("DUNGEON COMPLETE", 30, 12, PLAYER);
    viewport.centered_text("THE CROWN IS YOURS", 44, 6, PLAYER_ACCENT);
    let Some(run) = client.dungeon_v2_run.as_ref() else {
        return;
    };
    let dungeon = dungeon_v2_definition();
    let seconds = run.play_ticks / 60;
    let lines = [
        format!(
            "TIME             {}:{:02}:{:02}",
            seconds / 3600,
            (seconds / 60) % 60,
            seconds % 60
        ),
        format!("DEATHS           {}", run.deaths),
        format!(
            "COINS BANKED     {}/{}",
            run.inventory.coins.len(),
            dungeon_v2_total_coins()
        ),
        format!(
            "ROOMS EXPLORED   {}/{}",
            client.explored_v2_rooms.len(),
            dungeon.instances.len()
        ),
    ];
    for (index, line) in lines.iter().enumerate() {
        viewport.text(line, 82, 70 + index as i32 * 14, 6, UI_TEXT);
    }
    viewport.centered_text("ENTER  RETURN TO TITLE", 160, 6, UI_DIM);
}

fn draw_title_screen(viewport: &PixelViewport, menu: &TitleMenuState) {
    viewport.rectangle(CoreRect::new(0, 0, 320, 180), MENU_BACKGROUND);
    viewport.centered_text("DOWNWARDS", 44, 18, PLAYER);
    viewport.centered_text(
        "DESCEND · BANK COINS · CLAIM THE CROWN",
        60,
        6,
        PLAYER_ACCENT,
    );
    for (index, item) in menu.items().iter().enumerate() {
        let selected = index == menu.selected_index;
        let y = 92 + index as i32 * 14;
        if selected {
            viewport.rectangle(CoreRect::new(105, y - 9, 110, 12), MENU_SELECTED);
            viewport.rectangle_outline(CoreRect::new(105, y - 9, 110, 12), 1, PLAYER_ACCENT);
        }
        let label = match item {
            TitleMenuItem::Continue => "CONTINUE",
            TitleMenuItem::NewGame => {
                if menu.confirm_new_game {
                    "NEW GAME?"
                } else {
                    "NEW GAME"
                }
            }
            TitleMenuItem::Controls => "CONTROLS",
            TitleMenuItem::Quit => "QUIT",
        };
        viewport.centered_text(label, y, 7, if selected { PLAYER } else { UI_DIM });
    }
    if menu.confirm_new_game {
        viewport.centered_text(
            "THIS ARCHIVES YOUR RUN · ENTER AGAIN TO CONFIRM · ESC KEEPS IT",
            156,
            6,
            HAZARD,
        );
    }
    viewport.centered_text("UP/DOWN SELECT   ENTER CONFIRM", 170, 6, UI_DIM);
}

fn draw_controls_screen(viewport: &PixelViewport) {
    viewport.rectangle(CoreRect::new(0, 0, 320, 180), MENU_BACKGROUND);
    viewport.centered_text("CONTROLS", 14, 9, PLAYER);
    let dungeon_lines = [
        "MOVE       LEFT/RIGHT OR A/D",
        "JUMP       SPACE, Z, OR UP · TAP=LOW HOLD=HIGH",
        "WALL JUMP  HOLD TOWARD WALL + JUMP (NEEDS GLOVES)",
        "DASH       X OR SHIFT + A DIRECTION (NEEDS BOOTS)",
        "R          RESTART THE ROOM",
        "TAB        DUNGEON MAP",
        "F3         MUTE / UNMUTE MUSIC",
        "ESC        PAUSE MENU",
    ];
    for (index, line) in dungeon_lines.iter().enumerate() {
        viewport.text(line, 26, 32 + index as i32 * 12, 6, UI_TEXT);
    }
    viewport.text("LAB EXTRAS", 26, 126, 6, PLAYER_ACCENT);
    let lab_lines = [
        "V / C      AI ROUTE / AI COIN SOLVE",
        "H  P  N    REPLAY LAST ATTEMPT · PAUSE · STEP",
        "F1 / F2    DEBUG OVERLAY / MOVEMENT TUNING",
    ];
    for (index, line) in lab_lines.iter().enumerate() {
        viewport.text(line, 26, 138 + index as i32 * 10, 5, UI_DIM);
    }
    viewport.centered_text("ESC/ENTER BACK", 172, 6, UI_DIM);
}

fn draw_dungeon_v2_map(viewport: &PixelViewport, client: &ClientState) {
    const CELL_W: i32 = 12;
    const CELL_H: i32 = 8;
    const PITCH_X: i32 = 17;
    const PITCH_Y: i32 = 13;
    const CENTER_X: i32 = 160;
    const CENTER_Y: i32 = 92;

    let Some(run) = client.dungeon_v2_run.as_ref() else {
        return;
    };
    viewport.rectangle(CoreRect::new(0, 0, 320, 180), MENU_BACKGROUND);
    viewport.centered_text("DUNGEON MAP", 12, 9, PLAYER);
    viewport.centered_text(
        &format!(
            "COINS {}/{}   GOLD DOT = COINS LEFT   MATCHING MARKS = LINKED DOORS",
            run.inventory.coins.len(),
            dungeon_v2_total_coins()
        ),
        24,
        5,
        PLAYER_ACCENT,
    );

    let layout = dungeon_v2_map_layout();
    let dungeon = dungeon_v2_definition();
    let origin = layout[&run.room];
    let cell_origin = |instance_id: &str| {
        let (x, y) = layout[instance_id];
        (
            CENTER_X + (x - origin.0) * PITCH_X - CELL_W / 2,
            CENTER_Y + (y - origin.1) * PITCH_Y - CELL_H / 2,
        )
    };
    let on_screen =
        |(x, y): (i32, i32)| (-CELL_W..320).contains(&x) && (30 - CELL_H..168).contains(&y);

    // Links draw before cells. A straight stub is drawn ONLY when the two
    // rooms actually occupy adjacent map cells, so a stub touching a cell edge
    // always corresponds to a real door of that room. Connections whose rooms
    // were displaced apart by the layout draw as a pair of matching-colour
    // portal markers on the two door edges instead of a (lying) line. Sealed
    // doors the player cannot yet open use the hazard tint.
    const PORTAL_COLOURS: [Color; 6] = [
        Color::new(0.95, 0.55, 0.25, 1.0),
        Color::new(0.45, 0.85, 0.45, 1.0),
        Color::new(0.85, 0.45, 0.85, 1.0),
        Color::new(0.35, 0.82, 0.95, 1.0),
        Color::new(0.95, 0.85, 0.35, 1.0),
        Color::new(0.60, 0.60, 0.95, 1.0),
    ];
    let door_step = |door_id: &str| match door_id {
        "west" => (-1, 0),
        "east" => (1, 0),
        "ceiling" => (0, -1),
        _ => (0, 1),
    };
    let door_edge_midpoint = |instance_id: &str, door_id: &str| {
        let (x, y) = cell_origin(instance_id);
        match door_id {
            "west" => (x - 1, y + CELL_H / 2),
            "east" => (x + CELL_W, y + CELL_H / 2),
            "ceiling" => (x + CELL_W / 2, y - 1),
            _ => (x + CELL_W / 2, y + CELL_H),
        }
    };
    let mut portal_edges: Vec<(String, String)> = dungeon
        .instances
        .values()
        .flat_map(|instance| {
            instance
                .connections
                .keys()
                .map(move |door_id| (instance.id.clone(), door_id.clone()))
        })
        .collect();
    portal_edges.sort();
    let displaced_edges: Vec<(String, String)> = portal_edges
        .iter()
        .filter(|(instance_id, door_id)| {
            let (destination, mate_door) = &dungeon.instances[instance_id].connections[door_id];
            let (sx, sy) = layout[instance_id];
            let (dx, dy) = layout[destination];
            (dx - sx, dy - sy) != door_step(door_id)
                && (destination.as_str(), mate_door.as_str())
                    > (instance_id.as_str(), door_id.as_str())
        })
        .cloned()
        .collect();
    for (instance_id, door_id) in &portal_edges {
        if !client.explored_v2_rooms.contains(instance_id) {
            continue;
        }
        let (destination, mate_door) = &dungeon.instances[instance_id].connections[door_id];
        let sealed = dungeon_v2_door_requirement(instance_id, door_id)
            .is_some_and(|requirement| !run.inventory.satisfies(requirement))
            || dungeon_v2_door_requirement(destination, mate_door)
                .is_some_and(|requirement| !run.inventory.satisfies(requirement));
        let (sx, sy) = layout[instance_id];
        let (dx, dy) = layout[destination];
        let step = door_step(door_id);
        let adjacent = (dx - sx, dy - sy) == step;
        if adjacent {
            // Straight stub across the inter-cell gap; each explored-explored
            // pair draws it once, an explored-unexplored edge draws it as the
            // usual hint stub.
            if client.explored_v2_rooms.contains(destination) && destination < instance_id {
                continue;
            }
            let (x, y) = cell_origin(instance_id);
            let (stub_x, stub_y, stub_w, stub_h) = match door_id.as_str() {
                "west" => (x - (PITCH_X - CELL_W), y + CELL_H / 2, PITCH_X - CELL_W, 1),
                "east" => (x + CELL_W, y + CELL_H / 2, PITCH_X - CELL_W, 1),
                "ceiling" => (x + CELL_W / 2, y - (PITCH_Y - CELL_H), 1, PITCH_Y - CELL_H),
                _ => (x + CELL_W / 2, y + CELL_H, 1, PITCH_Y - CELL_H),
            };
            viewport.rectangle(
                CoreRect::new(stub_x, stub_y, stub_w, stub_h),
                if sealed { HAZARD_DARK } else { UI_DIM },
            );
        } else {
            // Displaced link: a portal marker on this room's door edge. The
            // colour is keyed to the canonical edge identity, so the marker on
            // the other room (drawn when that room is explored) matches.
            let canonical = if (destination.as_str(), mate_door.as_str())
                < (instance_id.as_str(), door_id.as_str())
            {
                (destination.clone(), mate_door.clone())
            } else {
                (instance_id.clone(), door_id.clone())
            };
            let palette_slot = displaced_edges
                .iter()
                .position(|edge| *edge == canonical)
                .expect("displaced edge is enumerated");
            let colour = if sealed {
                HAZARD_DARK
            } else {
                PORTAL_COLOURS[palette_slot % PORTAL_COLOURS.len()]
            };
            let (px, py) = door_edge_midpoint(instance_id, door_id);
            viewport.rectangle(CoreRect::new(px - 1, py - 1, 3, 3), colour);
        }
    }

    for instance_id in &client.explored_v2_rooms {
        let (x, y) = cell_origin(instance_id);
        if !on_screen((x, y)) {
            continue;
        }
        let current = *instance_id == run.room;
        // The spawn room doubles as the exit node: always outlined in the
        // exit colour, filled bright once the crown is held and the escape
        // gate is live.
        let is_exit_node = *instance_id == dungeon.spawn;
        viewport.rectangle(
            CoreRect::new(x, y, CELL_W, CELL_H),
            if current {
                MENU_SELECTED
            } else if is_exit_node && run.inventory.crown {
                EXIT_DARK
            } else {
                DEBUG_PANEL
            },
        );
        viewport.rectangle_outline(
            CoreRect::new(x, y, CELL_W, CELL_H),
            1,
            if is_exit_node {
                EXIT
            } else if current {
                PLAYER_ACCENT
            } else {
                UI_DIM
            },
        );
        if is_exit_node {
            // Door glyph on the cell's west edge, mirroring the in-room gate.
            viewport.rectangle(CoreRect::new(x + 1, y + 2, 2, CELL_H - 4), EXIT);
        }
        let remaining = dungeon_v2_room(instance_id, &run.inventory)
            .pickups()
            .iter()
            .filter(|pickup| pickup.id().starts_with("v2-coin-"))
            .count();
        if remaining > 0 {
            viewport.rectangle(CoreRect::new(x + CELL_W - 4, y + 1, 3, 3), PICKUP);
        }
    }

    // Exit legend, drawn after the cells so it stays legible.
    viewport.centered_text(
        if run.inventory.crown {
            "THE CROWN IS YOURS · ESCAPE THROUGH THE GREEN EXIT DOOR"
        } else {
            "GREEN DOOR = EXIT · RETURN THERE WITH THE CROWN"
        },
        160,
        5,
        if run.inventory.crown { EXIT } else { UI_DIM },
    );
    viewport.centered_text(&run.room.to_ascii_uppercase(), 168, 7, UI_TEXT);
    viewport.centered_text("TAB/ESC CLOSE", 177, 5, UI_DIM);
}

fn draw_dungeon_map(viewport: &PixelViewport, client: &ClientState) {
    const CELL_W: i32 = 12;
    const CELL_H: i32 = 8;
    const PITCH_X: i32 = 17;
    const PITCH_Y: i32 = 13;
    const CENTER_X: i32 = 160;
    const CENTER_Y: i32 = 92;

    let Some(run) = client.dungeon_run else {
        return;
    };
    viewport.rectangle(CoreRect::new(0, 0, 320, 180), MENU_BACKGROUND);
    viewport.centered_text("DUNGEON MAP", 12, 9, PLAYER);
    viewport.centered_text(
        &format!(
            "COINS {}/{}   GOLD DOT = COINS LEFT IN A ROOM YOU HAVE SEEN",
            run.inventory.coin_count(),
            DEMO_DUNGEON_TOTAL_COINS
        ),
        24,
        5,
        PLAYER_ACCENT,
    );

    let layout = dungeon_map_layout();
    let definition = demo_dungeon_definition();
    let origin = layout[&run.room];
    let cell_origin = |room: DemoDungeonRoom| {
        let (x, y) = layout[&room];
        (
            CENTER_X + (x - origin.0) * PITCH_X - CELL_W / 2,
            CENTER_Y + (y - origin.1) * PITCH_Y - CELL_H / 2,
        )
    };
    let on_screen =
        |(x, y): (i32, i32)| (-CELL_W..320).contains(&x) && (30 - CELL_H..168).contains(&y);

    // Connection stubs between explored neighbours first, so cells draw over
    // them.
    for &room in &client.explored_rooms {
        let (x, y) = cell_origin(room);
        if !on_screen((x, y)) {
            continue;
        }
        let floor = definition
            .floor(room.authored_key())
            .expect("room has a floor definition");
        for connection in &floor.connections {
            let (stub_x, stub_y, stub_w, stub_h) = match connection.door_id.as_str() {
                "west" => (x - (PITCH_X - CELL_W), y + CELL_H / 2, PITCH_X - CELL_W, 1),
                "east" => (x + CELL_W, y + CELL_H / 2, PITCH_X - CELL_W, 1),
                "ceiling" => (x + CELL_W / 2, y - (PITCH_Y - CELL_H), 1, PITCH_Y - CELL_H),
                _ => (x + CELL_W / 2, y + CELL_H, 1, PITCH_Y - CELL_H),
            };
            viewport.rectangle(CoreRect::new(stub_x, stub_y, stub_w, stub_h), UI_DIM);
        }
    }

    for &room in &client.explored_rooms {
        let (x, y) = cell_origin(room);
        if !on_screen((x, y)) {
            continue;
        }
        let current = room == run.room;
        viewport.rectangle(
            CoreRect::new(x, y, CELL_W, CELL_H),
            if current { MENU_SELECTED } else { DEBUG_PANEL },
        );
        viewport.rectangle_outline(
            CoreRect::new(x, y, CELL_W, CELL_H),
            1,
            if current { PLAYER_ACCENT } else { UI_DIM },
        );
        let floor = definition
            .floor(room.authored_key())
            .expect("room has a floor definition");
        let remaining = floor
            .coin_indices
            .iter()
            .filter(|index| !run.inventory.has_coin(&format!("dungeon-coin-{index:02}")))
            .count();
        if remaining > 0 {
            viewport.rectangle(CoreRect::new(x + CELL_W - 4, y + 1, 3, 3), PICKUP);
        }
    }

    viewport.centered_text(&run.room.title().to_ascii_uppercase(), 168, 7, UI_TEXT);
    viewport.centered_text("TAB/ESC CLOSE", 177, 5, UI_DIM);
}

fn draw_dungeon_floor_menu(
    viewport: &PixelViewport,
    client: &ClientState,
    visual_assets: &VisualAssets,
) {
    const PREVIEW_X: i32 = 171;
    const PREVIEW_Y: i32 = 43;
    const PREVIEW_SCALE: f32 = 0.43;

    viewport.rectangle(CoreRect::new(0, 0, 320, 180), MENU_BACKGROUND);
    viewport.centered_text("DOWNWARDS  /  DUNGEON FLOOR LAB", 12, 9, PLAYER);
    viewport.centered_text(
        &format!(
            "{} AUTHORED FLOORS / NON-PERSISTENT ANALYSIS LOADOUTS",
            client.gallery_menu.item_count
        ),
        24,
        5,
        PLAYER_ACCENT,
    );

    let visible = client.gallery_menu.visible_indices();
    let visible_end = visible.end;
    for (row, index) in visible.enumerate() {
        let y = 42 + row as i32 * 11;
        let Some(room) = DemoDungeonRoom::ALL.get(index).copied() else {
            continue;
        };
        if index == client.gallery_menu.selected_index {
            viewport.rectangle(CoreRect::new(4, y - 8, 159, 10), MENU_SELECTED);
            viewport.rectangle_outline(CoreRect::new(4, y - 8, 159, 10), 1, PLAYER_ACCENT);
            viewport.text(">", 6, y, 6, PLAYER_ACCENT);
        }
        viewport.text(&format!("{:>3}", index + 1), 13, y, 5, UI_DIM);
        viewport.text(room.title(), 32, y, 6, UI_TEXT);
    }
    if client.gallery_menu.first_visible_index > 0 {
        viewport.text("^", 164, 42, 6, PLAYER_ACCENT);
    }
    if visible_end < client.gallery_menu.item_count {
        viewport.text("v", 164, 141, 6, PLAYER_ACCENT);
    }

    viewport.rectangle(CoreRect::new(168, 38, 146, 84), DEBUG_PANEL);
    viewport.rectangle_outline(CoreRect::new(168, 38, 146, 84), 1, UI_DIM);
    if let Some(simulation) = &client.level_menu_preview.simulation {
        viewport.rectangle(
            CoreRect::new(PREVIEW_X, PREVIEW_Y, 138, 78),
            ROOM_BACKGROUND,
        );
        let preview_viewport = PixelViewport {
            scale: viewport.scale * PREVIEW_SCALE,
            left: viewport.screen_x(PREVIEW_X),
            top: viewport.screen_y(PREVIEW_Y),
        };
        draw_tiles(&preview_viewport, simulation.room(), visual_assets);
        draw_room_objects(&preview_viewport, simulation, visual_assets);
        draw_exits(&preview_viewport, simulation.room(), None, visual_assets);
        draw_animated_player(
            &preview_viewport,
            simulation.player().bounds(),
            PlayerVisual {
                pose: PlayerPose::IdleA,
                flip_x: false,
            },
            visual_assets,
        );
    } else {
        viewport.centered_text_in(
            CoreRect::new(168, 38, 146, 84),
            "PREVIEW UNAVAILABLE",
            78,
            7,
            HAZARD,
        );
    }

    if let Some(selection) = client.selected_browser_scenario()
        && let Some(room) = DemoDungeonRoom::ALL
            .get(client.gallery_menu.selected_index)
            .copied()
    {
        let spec = dungeon_floor_route_spec(room);
        let stats = client.stats_for(selection);
        viewport.text(
            &fit_preview_line(&format!(
                "{} / {}",
                room.id().to_ascii_uppercase(),
                room.title().to_ascii_uppercase()
            )),
            170,
            132,
            5,
            PLAYER,
        );
        viewport.text(
            &fit_preview_line(&format!(
                "TARGET {} {}",
                spec.target.kind().to_ascii_uppercase(),
                spec.target.id().to_ascii_uppercase()
            )),
            170,
            139,
            5,
            WALL_SLIDE_CUE,
        );
        let abilities = spec.inventory.abilities();
        viewport.text(
            &fit_preview_line(&format!(
                "WALL:{} DASH:{} / V TRACTABILITY WITNESS",
                if abilities.wall_jump { "ON" } else { "LOCKED" },
                if abilities.dash { "ON" } else { "LOCKED" }
            )),
            170,
            146,
            5,
            PLAYER_ACCENT,
        );
        viewport.text(
            &fit_preview_line(&format!(
                "RUNS {} D {} C {} BEST {}",
                stats.attempts,
                stats.deaths,
                stats.clears,
                format_best_clear(stats.best_clear_ticks)
            )),
            170,
            153,
            5,
            UI_TEXT,
        );
    } else {
        viewport.centered_text("NO DUNGEON FLOORS REGISTERED", 137, 7, HAZARD);
    }
    if let Some(error) = &client.level_menu_preview.error {
        viewport.text(&fit_preview_line(error), 170, 153, 5, HAZARD);
    }

    viewport.centered_text(
        "ARROWS/WASD PREVIOUS/NEXT   ENTER PLAY   V WITNESS",
        164,
        6,
        UI_TEXT,
    );
    let scroll_hint = if visible_end < client.gallery_menu.item_count {
        "   v MORE BELOW"
    } else if client.gallery_menu.first_visible_index > 0 {
        "   ^ MORE ABOVE"
    } else {
        ""
    };
    viewport.centered_text(
        &format!(
            "FLOOR {}/{}   HOME/END FIRST/LAST{scroll_hint}",
            client.gallery_menu.selected_index.saturating_add(1),
            client.gallery_menu.item_count
        ),
        174,
        6,
        UI_DIM,
    );
}

fn draw_level_menu(viewport: &PixelViewport, client: &ClientState, visual_assets: &VisualAssets) {
    viewport.rectangle(CoreRect::new(0, 0, 320, 180), MENU_BACKGROUND);
    viewport.centered_text("DOWNWARDS  /  LEVEL SELECT", 12, 9, PLAYER);
    for (index, tier) in [
        AbilityTier::Baseline,
        AbilityTier::WallJump,
        AbilityTier::Dash,
        AbilityTier::WallJumpAndDash,
    ]
    .into_iter()
    .enumerate()
    {
        let bounds = CoreRect::new(3 + index as i32 * 79, 17, 77, 15);
        let selected = tier == client.level_menu.tier;
        viewport.rectangle(bounds, if selected { MENU_SELECTED } else { DEBUG_PANEL });
        viewport.rectangle_outline(
            bounds,
            if selected { 2 } else { 1 },
            if selected { PLAYER_ACCENT } else { UI_DIM },
        );
        viewport.centered_text_in(
            bounds,
            kit_tab_label(tier),
            28,
            6,
            if selected { PLAYER } else { UI_DIM },
        );
    }
    viewport.centered_text(
        &format!(
            "{}   /   {} NAMES v{}   /   {} CURATED + DEV",
            compact_ability_summary(client.level_menu.tier),
            if client.catalogue.is_provisional() {
                "CORPUS PROVISIONAL"
            } else if client.catalogue.is_corpus() {
                "CORPUS FINAL"
            } else {
                "GEN v6"
            },
            LEVEL_IDENTIFIER_VERSION,
            client.catalogue.entries(client.level_menu.tier).len()
        ),
        40,
        5,
        PLAYER_ACCENT,
    );

    let visible = client.level_menu.visible_indices();
    let visible_end = visible.end;
    for (row, index) in visible.enumerate() {
        let y = 52 + row as i32 * 12;
        if index == client.level_menu.selected_index {
            viewport.rectangle(CoreRect::new(4, y - 8, 159, 10), MENU_SELECTED);
            viewport.rectangle_outline(CoreRect::new(4, y - 8, 159, 10), 1, PLAYER_ACCENT);
            viewport.text(">", 6, y, 6, PLAYER_ACCENT);
        }
        let curated_count = client.catalogue.entries(client.level_menu.tier).len();
        if index == curated_count {
            viewport.text(DEVELOPMENT_LEVEL_IDENTIFIER, 13, y, 6, UI_TEXT);
            viewport.text("DEV", 145, y, 5, UI_DIM);
        } else {
            let entry = client
                .catalogue
                .entry(client.level_menu.tier, index)
                .expect("menu rows are bounded by the curated manifest");
            viewport.text(client.catalogue.name(entry), 13, y, 6, UI_TEXT);
            let (kind, colour) = entry.legacy().map_or_else(
                || {
                    (
                        match entry.corpus().expect("entry kind").key().generator_slug() {
                            "partition-route" => "PART".to_owned(),
                            "compositional-route-cut" => "CUT".to_owned(),
                            "compositional-ability" => "ABILITY".to_owned(),
                            _ => "CORPUS".to_owned(),
                        },
                        PLAYER_ACCENT,
                    )
                },
                |legacy| {
                    (
                        format!(
                            "{} {}",
                            compact_band_label(legacy.band()),
                            compact_strategy_label(legacy.key().profile.strategy)
                        ),
                        band_colour(legacy.band()),
                    )
                },
            );
            viewport.text(&kind, 116, y, 5, colour);
            viewport.text(&format!("{:02}", entry.index() + 1), 151, y, 5, UI_DIM);
        }
    }
    if client.level_menu.first_visible_index > 0 {
        viewport.text("^", 164, 52, 6, PLAYER_ACCENT);
    }
    if visible_end < client.level_menu.item_count {
        viewport.text("v", 164, 148, 6, PLAYER_ACCENT);
    }

    draw_level_menu_preview(viewport, client, visual_assets);

    viewport.centered_text(
        "UP/DOWN LEVELS   LEFT/RIGHT KITS   ENTER PLAY",
        164,
        6,
        UI_TEXT,
    );
    viewport.centered_text(
        if client.catalogue.is_corpus() {
            "1-4 KIT SHORTCUT   C LIVE COIN SOLVE   ESC/M RETURN"
        } else {
            "1-4 KIT SHORTCUT   C COIN ROUTE   ESC/M RETURN"
        },
        174,
        6,
        UI_DIM,
    );
}

fn draw_level_menu_preview(
    viewport: &PixelViewport,
    client: &ClientState,
    visual_assets: &VisualAssets,
) {
    const PREVIEW_X: i32 = 171;
    const PREVIEW_Y: i32 = 45;
    const PREVIEW_SCALE: f32 = 0.43;

    viewport.rectangle(CoreRect::new(168, 42, 146, 84), DEBUG_PANEL);
    viewport.rectangle_outline(CoreRect::new(168, 42, 146, 84), 1, UI_DIM);
    if let Some(simulation) = &client.level_menu_preview.simulation {
        viewport.rectangle(
            CoreRect::new(PREVIEW_X, PREVIEW_Y, 138, 78),
            ROOM_BACKGROUND,
        );
        let preview_viewport = PixelViewport {
            scale: viewport.scale * PREVIEW_SCALE,
            left: viewport.screen_x(PREVIEW_X),
            top: viewport.screen_y(PREVIEW_Y),
        };
        draw_tiles(&preview_viewport, simulation.room(), visual_assets);
        draw_room_objects(&preview_viewport, simulation, visual_assets);
        let focus = client
            .catalogue_entry(client.level_menu.selected_scenario())
            .map(|entry| DoorFocus {
                source: entry.source_door_id(),
                target: entry.target_door_id(),
            });
        draw_exits(&preview_viewport, simulation.room(), focus, visual_assets);
        draw_animated_player(
            &preview_viewport,
            simulation.player().bounds(),
            PlayerVisual {
                pose: PlayerPose::IdleA,
                flip_x: false,
            },
            visual_assets,
        );
    } else {
        viewport.centered_text_in(
            CoreRect::new(168, 42, 146, 84),
            "PREVIEW UNAVAILABLE",
            82,
            7,
            HAZARD,
        );
    }

    let selection = client.level_menu.selected_scenario();
    let stats = client.stats_for(selection);
    let (metadata, route, mechanics) = match selection.mode {
        RoomMode::Development => (
            "FIXED DEVELOPMENT MECHANICS ROOM".to_owned(),
            "ANY EXIT COMPLETES THIS SPECIAL ROOM".to_owned(),
            verified_route_mechanics(selection.tier, None),
        ),
        RoomMode::Gallery => client.gallery_entry(selection).map_or_else(
            || {
                (
                    "MISSING GALLERY ENTRY".to_owned(),
                    "EXACT ROUTE unavailable".to_owned(),
                    "CONTENT-LOCKED NO-DASH LOADOUT".to_owned(),
                )
            },
            |level| {
                (
                    format!("{} / {}", level.id(), level.title()),
                    format!("EXACT STORED ROUTE -> {}", level.target()),
                    level.mechanic_axis().to_owned(),
                )
            },
        ),
        RoomMode::CalibratedGenerated => client.calibrated_entry(selection).map_or_else(
            || {
                (
                    "MISSING CALIBRATED ENTRY".to_owned(),
                    "EXACT ROUTE unavailable".to_owned(),
                    "CONTENT-LOCKED WALL-JUMP LOADOUT".to_owned(),
                )
            },
            |level| {
                (
                    format!(
                        "GENERATED V{} / SEED {:02}",
                        CALIBRATED_WALL_JUMP_GENERATION_VERSION,
                        level.seed()
                    ),
                    format!("SIMPLIFIED STORED ROUTE -> {}", level.target()),
                    level.mechanic_axis().to_owned(),
                )
            },
        ),
        RoomMode::Challenge(kind) => (
            format!(
                "FIXED HAND-AUTHORED {} VALIDATION CHALLENGE",
                kind.difficulty_label()
            ),
            format!("EXACT STORED ROUTE -> {}", kind.target()),
            "KIT WALL:ON DASH:LOCKED | V PLAYS EXACT WITNESS".to_owned(),
        ),
        RoomMode::Dungeon => (
            "PERSISTENT SEVEN-ROOM VERTICAL SLICE".to_owned(),
            "FIND BOOTS · CROSS GALE CHASM · CLAIM CROWN".to_owned(),
            "ROOM EXITS TRAVERSE THE DUNGEON GRAPH".to_owned(),
        ),
        RoomMode::DungeonV2 => (
            "REDESIGNED DATA-DRIVEN DUNGEON".to_owned(),
            "FIND GLOVES AND BOOTS · BANK COINS · CLAIM CROWN".to_owned(),
            "ROOM EXITS TRAVERSE THE DUNGEON GRAPH".to_owned(),
        ),
        RoomMode::Generated => client.catalogue_entry(selection).map_or_else(
            || {
                (
                    "MISSING CURATED ENTRY".to_owned(),
                    "ROUTE ? -> ?".to_owned(),
                    verified_route_mechanics(selection.tier, None),
                )
            },
            |entry| match entry.legacy() {
                Some(legacy) => {
                    let profile = legacy.key().profile;
                    (
                        format!(
                            "v6 {} / INTENT {} / VIS {:08X}",
                            profile.strategy.slug(),
                            profile.intent.slug(),
                            legacy.visual_fingerprint() >> 32
                        ),
                        format!(
                            "AI-{} ROUTE {} -> {}",
                            catalogue_band_label(legacy.band()),
                            entry.source_door_id(),
                            entry.target_door_id()
                        ),
                        verified_route_mechanics(
                            selection.tier,
                            Some((legacy.successful_wall_jumps(), legacy.successful_dashes())),
                        ),
                    )
                }
                None => {
                    let corpus = entry.corpus().expect("entry kind");
                    (
                        format!(
                            "CORPUS {} / SEED {:X}",
                            corpus.key().generator_slug(),
                            corpus.key().source_seed()
                        ),
                        format!(
                            "CERTIFIED ROUTE {} -> {}",
                            entry.source_door_id(),
                            entry.target_door_id()
                        ),
                        verified_route_mechanics(selection.tier, None),
                    )
                }
            },
        ),
        RoomMode::DeveloperGenerated => (
            "EXPLICIT UNCURATED DEVELOPER SEED".to_owned(),
            "AI ROUTE CHOSEN AT RUNTIME".to_owned(),
            verified_route_mechanics(selection.tier, None),
        ),
    };
    viewport.text(&fit_preview_line(&metadata), 170, 132, 5, UI_TEXT);
    viewport.text(&fit_preview_line(&route), 170, 139, 5, PLAYER_ACCENT);
    viewport.text(&fit_preview_line(&mechanics), 170, 146, 5, WALL_SLIDE_CUE);
    viewport.text(
        &fit_preview_line(&format!(
            "RUNS {} D {} C {} COINS {} BEST {}",
            stats.attempts,
            stats.deaths,
            stats.clears,
            stats.coins_collected,
            format_best_clear(stats.best_clear_ticks)
        )),
        170,
        153,
        5,
        PLAYER_ACCENT,
    );
    if let Some(error) = &client.level_menu_preview.error {
        viewport.text(&fit_preview_line(error), 170, 153, 5, HAZARD);
    }
}

fn draw_environment_sprite(
    viewport: &PixelViewport,
    bounds: CoreRect,
    sprite: EnvironmentSprite,
    assets: &VisualAssets,
    flip_x: bool,
) {
    draw_texture_ex(
        &assets.environment_tiles,
        viewport.screen_x(bounds.x),
        viewport.screen_y(bounds.y),
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(
                bounds.width as f32 * viewport.scale,
                bounds.height as f32 * viewport.scale,
            )),
            source: Some(sprite.sheet_source()),
            flip_x,
            ..Default::default()
        },
    );
}

fn draw_tiles(viewport: &PixelViewport, room: &Room, assets: &VisualAssets) {
    for y in 0..room.height() {
        for x in 0..room.width() {
            let bounds = room.tile_bounds(x, y);
            match room.tile(x, y).expect("coordinates came from room bounds") {
                Tile::Empty => {}
                Tile::Solid => {
                    viewport.rectangle(bounds, SOLID);
                    draw_environment_sprite(
                        viewport,
                        bounds,
                        if (x + y).is_multiple_of(2) {
                            EnvironmentSprite::SolidA
                        } else {
                            EnvironmentSprite::SolidB
                        },
                        assets,
                        false,
                    );
                    draw_solid_exposed_edges(viewport, room, x, y, bounds);
                }
                Tile::HazardUp | Tile::HazardDown | Tile::HazardLeft | Tile::HazardRight => {
                    draw_spikes(
                        viewport,
                        bounds,
                        room.hazard_direction(x, y)
                            .expect("hazard tile has a shared direction"),
                        assets,
                    )
                }
                Tile::OneWay => draw_one_way(viewport, bounds, assets),
            }
        }
    }
    draw_paired_spike_bases(viewport, room);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpikeBasePair {
    Horizontal,
    Vertical,
}

fn spike_base_pair(room: &Room, x: u16, y: u16) -> Option<SpikeBasePair> {
    match room.hazard_direction(x, y)? {
        HazardDirection::Up
            if y + 1 < room.height()
                && room.hazard_direction(x, y + 1) == Some(HazardDirection::Down) =>
        {
            Some(SpikeBasePair::Horizontal)
        }
        HazardDirection::Left
            if x + 1 < room.width()
                && room.hazard_direction(x + 1, y) == Some(HazardDirection::Right) =>
        {
            Some(SpikeBasePair::Vertical)
        }
        HazardDirection::Up
        | HazardDirection::Down
        | HazardDirection::Left
        | HazardDirection::Right => None,
    }
}

fn draw_paired_spike_bases(viewport: &PixelViewport, room: &Room) {
    for y in 0..room.height() {
        for x in 0..room.width() {
            let bounds = room.tile_bounds(x, y);
            match spike_base_pair(room, x, y) {
                Some(SpikeBasePair::Horizontal) => viewport.rectangle(
                    CoreRect::new(bounds.x, bounds.bottom() - 1, bounds.width, 2),
                    SPIKE_BASE,
                ),
                Some(SpikeBasePair::Vertical) => viewport.rectangle(
                    CoreRect::new(bounds.right() - 1, bounds.y, 2, bounds.height),
                    SPIKE_BASE,
                ),
                None => {}
            }
        }
    }
}

fn solid_at_offset(room: &Room, x: u16, y: u16, dx: i32, dy: i32) -> bool {
    let Ok(target_x) = u16::try_from(i32::from(x) + dx) else {
        return false;
    };
    let Ok(target_y) = u16::try_from(i32::from(y) + dy) else {
        return false;
    };
    room.tile(target_x, target_y) == Some(Tile::Solid)
}

fn draw_solid_exposed_edges(
    viewport: &PixelViewport,
    room: &Room,
    x: u16,
    y: u16,
    bounds: CoreRect,
) {
    if !solid_at_offset(room, x, y, 0, -1) {
        viewport.rectangle(
            CoreRect::new(bounds.x, bounds.y, bounds.width, 1),
            SOLID_EXPOSED_TOP,
        );
    }
    if !solid_at_offset(room, x, y, -1, 0) {
        viewport.rectangle(
            CoreRect::new(bounds.x, bounds.y, 1, bounds.height),
            SOLID_EXPOSED_SIDE,
        );
    }
    if !solid_at_offset(room, x, y, 1, 0) {
        viewport.rectangle(
            CoreRect::new(bounds.right() - 1, bounds.y, 1, bounds.height),
            SOLID_EXPOSED_SIDE,
        );
    }
    if !solid_at_offset(room, x, y, 0, 1) {
        viewport.rectangle(
            CoreRect::new(bounds.x, bounds.bottom() - 1, bounds.width, 1),
            SOLID_EXPOSED_SIDE,
        );
    }
}

fn draw_one_way(viewport: &PixelViewport, bounds: CoreRect, assets: &VisualAssets) {
    draw_environment_sprite(viewport, bounds, EnvironmentSprite::OneWay, assets, false);
}

fn draw_spikes(
    viewport: &PixelViewport,
    bounds: CoreRect,
    direction: HazardDirection,
    assets: &VisualAssets,
) {
    let (sprite, flip_x) = match direction {
        HazardDirection::Up => (EnvironmentSprite::SpikesUp, false),
        HazardDirection::Down => (EnvironmentSprite::SpikesDown, false),
        HazardDirection::Left => (EnvironmentSprite::SpikesHorizontal, true),
        HazardDirection::Right => (EnvironmentSprite::SpikesHorizontal, false),
    };
    draw_environment_sprite(viewport, bounds, sprite, assets, flip_x);
}

/// Ticks of visible wind-up before a timed hazard turns deadly (~0.4s).
const TIMED_HAZARD_WARNING_TICKS: u64 = 24;

#[derive(Clone, Copy, PartialEq)]
enum TimedHazardVisual {
    Idle,
    /// About to turn deadly; progress runs 0.0 -> 1.0 up to the flip.
    Arming {
        progress: f32,
    },
    Deadly,
}

/// Deliberately procedural stand-in for the timed shutters: chunky diagonal
/// hazard stripes that stay crisp at any bounds size, until real pixel art
/// exists for large timed hazards. Deadly is a solid unmistakable fill;
/// the wind-up brightens amber so the flip is never a surprise.
fn draw_timed_hazard_placeholder(
    viewport: &PixelViewport,
    bounds: CoreRect,
    state: TimedHazardVisual,
) {
    const STRIPE_PERIOD: i32 = 16;
    const STRIPE_WIDTH: i32 = 8;
    const ROW_STEP: i32 = 2;
    let (stripe, outline) = match state {
        TimedHazardVisual::Deadly => {
            // Solid lethal fill first so no part of the zone reads as safe.
            viewport.rectangle(bounds, Color::new(0.72, 0.16, 0.22, 1.0));
            (HAZARD, HAZARD)
        }
        TimedHazardVisual::Arming { progress } => {
            let ramp = progress.clamp(0.0, 1.0);
            let amber = Color::new(0.95, 0.65 + 0.15 * ramp, 0.15, 0.35 + 0.65 * ramp);
            (amber, amber)
        }
        TimedHazardVisual::Idle => (HAZARD_DARK, HAZARD_DARK),
    };
    let mut row = 0;
    while row < bounds.height {
        let height = ROW_STEP.min(bounds.height - row);
        let mut start = -((row + bounds.y) % STRIPE_PERIOD) - STRIPE_PERIOD;
        while start < bounds.width {
            let left = start.max(0);
            let right = (start + STRIPE_WIDTH).min(bounds.width);
            if right > left {
                viewport.rectangle(
                    CoreRect::new(bounds.x + left, bounds.y + row, right - left, height),
                    stripe,
                );
            }
            start += STRIPE_PERIOD;
        }
        row += ROW_STEP;
    }
    viewport.rectangle_outline(bounds, 1, outline);
}

fn draw_room_objects(viewport: &PixelViewport, simulation: &Simulation, assets: &VisualAssets) {
    for hazard in simulation.room().timed_hazards() {
        let period = u64::from(hazard.period_ticks());
        let cycle = (simulation.room_tick() % period + u64::from(hazard.phase_ticks())) % period;
        let state = if cycle < u64::from(hazard.active_ticks()) {
            TimedHazardVisual::Deadly
        } else {
            let until_active = period - cycle;
            if until_active <= TIMED_HAZARD_WARNING_TICKS {
                TimedHazardVisual::Arming {
                    // 0.0 at the start of the wind-up, 1.0 at the flip.
                    progress: 1.0 - until_active as f32 / TIMED_HAZARD_WARNING_TICKS as f32,
                }
            } else {
                TimedHazardVisual::Idle
            }
        };
        draw_timed_hazard_placeholder(viewport, hazard.bounds(), state);
    }

    for (index, pickup) in simulation.room().pickups().iter().enumerate() {
        if simulation.pickup_is_collected(index) == Some(true) {
            continue;
        }
        match pickup.id() {
            DEMO_DUNGEON_BOOT_PICKUP => draw_dungeon_pickup(
                viewport,
                pickup.bounds(),
                DungeonPickupSprite::WingedBoots,
                assets,
            ),
            DEMO_DUNGEON_CROWN_PICKUP => draw_dungeon_pickup(
                viewport,
                pickup.bounds(),
                DungeonPickupSprite::Crown,
                assets,
            ),
            _ => draw_environment_sprite(
                viewport,
                pickup.bounds(),
                EnvironmentSprite::Pickup,
                assets,
                false,
            ),
        }
    }
}

#[derive(Clone, Copy)]
enum DungeonPickupSprite {
    WingedBoots = 0,
    Crown = 1,
}

impl DungeonPickupSprite {
    const fn sheet_source(self) -> Rect {
        Rect::new(
            (self as u8) as f32 * DUNGEON_PICKUP_CELL_PIXELS,
            0.0,
            DUNGEON_PICKUP_CELL_PIXELS,
            DUNGEON_PICKUP_CELL_PIXELS,
        )
    }
}

fn draw_dungeon_pickup(
    viewport: &PixelViewport,
    bounds: CoreRect,
    sprite: DungeonPickupSprite,
    assets: &VisualAssets,
) {
    draw_texture_ex(
        &assets.dungeon_pickups,
        viewport.screen_x(bounds.x),
        viewport.screen_y(bounds.y),
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(
                bounds.width as f32 * viewport.scale,
                bounds.height as f32 * viewport.scale,
            )),
            source: Some(sprite.sheet_source()),
            ..Default::default()
        },
    );
}

#[derive(Clone, Copy)]
struct DoorFocus<'a> {
    source: &'a str,
    target: &'a str,
}

fn draw_exits(
    viewport: &PixelViewport,
    room: &Room,
    focus: Option<DoorFocus<'_>>,
    assets: &VisualAssets,
) {
    for exit in room.exits() {
        if exit.id == DEMO_DUNGEON_GOAL_EXIT {
            // The Crown is the goal's complete visual affordance. Its trigger deliberately
            // surrounds the pickup, but drawing a generic exit frame over the sprite would make
            // it look like another door and obscure the generated pixel art.
            continue;
        }
        viewport.rectangle(exit.bounds, EXIT_DARK);
        draw_environment_sprite(
            viewport,
            exit.bounds,
            EnvironmentSprite::Exit,
            assets,
            false,
        );
        viewport.rectangle_outline(exit.bounds, 1, EXIT);

        let centre_y = exit.bounds.y + exit.bounds.height / 2;
        let arrow_x = exit.bounds.x + 2;
        viewport.triangle(
            (arrow_x, centre_y - 3),
            (arrow_x + 3, centre_y),
            (arrow_x, centre_y + 3),
            EXIT,
        );
    }

    for door in room.doors() {
        let bounds = door.trigger_bounds;
        let colour = match focus {
            Some(focus) if door.id == focus.target => TARGET_DOOR,
            Some(focus) if door.id == focus.source => SOURCE_DOOR,
            _ => DOOR,
        };
        let centre_x = bounds.x + bounds.width / 2;
        let centre_y = bounds.y + bounds.height / 2;
        draw_environment_sprite(viewport, bounds, EnvironmentSprite::Door, assets, false);
        if focus.is_some_and(|focus| door.id == focus.target)
            && bounds.width > 2
            && bounds.height > 2
        {
            // A small focus pip replaces the old trigger-sized nested boxes.
            viewport.rectangle(CoreRect::new(centre_x - 1, bounds.y, 3, 1), colour);
        }
        match door.side {
            BoundarySide::Left => viewport.triangle(
                (bounds.x + 1, centre_y),
                (bounds.x + 5, centre_y - 3),
                (bounds.x + 5, centre_y + 3),
                colour,
            ),
            BoundarySide::Right => viewport.triangle(
                (bounds.right() - 1, centre_y),
                (bounds.right() - 5, centre_y - 3),
                (bounds.right() - 5, centre_y + 3),
                colour,
            ),
            BoundarySide::Ceiling => viewport.triangle(
                (centre_x, bounds.y + 1),
                (centre_x - 3, bounds.y + 5),
                (centre_x + 3, bounds.y + 5),
                colour,
            ),
            BoundarySide::Floor => viewport.triangle(
                (centre_x, bounds.bottom() - 1),
                (centre_x - 3, bounds.bottom() - 5),
                (centre_x + 3, bounds.bottom() - 5),
                colour,
            ),
        }
    }
}

fn draw_dungeon_coin_gates(viewport: &PixelViewport, client: &ClientState) {
    let Some(run) = client.dungeon_run else {
        return;
    };
    for door in client.simulation.room().doors() {
        let requirement = demo_dungeon_door_requirement(run.room, &door.id);
        if requirement.is_empty()
            || requirement.is_satisfied_by(&run.inventory.authored_progression_inventory())
        {
            continue;
        }
        let bounds = door.trigger_bounds;
        viewport.rectangle(bounds, Color::new(0.2, 0.04, 0.08, 0.82));
        for offset in [5, 14, 23, 32] {
            let slat = match door.side {
                BoundarySide::Left | BoundarySide::Right => {
                    CoreRect::new(bounds.x, bounds.y + offset, bounds.width, 3)
                }
                BoundarySide::Ceiling | BoundarySide::Floor => {
                    CoreRect::new(bounds.x + offset, bounds.y, 3, bounds.height)
                }
            };
            viewport.rectangle(slat, HAZARD);
        }
        let (label_x, label_y) = match door.side {
            BoundarySide::Right => (bounds.x - 38, bounds.y - 3),
            BoundarySide::Left => (bounds.right() + 3, bounds.y - 3),
            BoundarySide::Ceiling => (bounds.x + 2, bounds.bottom() + 6),
            BoundarySide::Floor => (bounds.x + 2, bounds.y - 3),
        };
        viewport.text(
            &dungeon_requirement_label(requirement),
            label_x,
            label_y,
            5,
            PICKUP,
        );
    }
}

fn dungeon_requirement_label(requirement: AuthoredDoorRequirement) -> String {
    let needs_wall = requirement
        .traversal_methods
        .contains(TraversalMethod::WallJump);
    let needs_dash = requirement
        .traversal_methods
        .contains(TraversalMethod::Dash);
    match (requirement.coins, needs_wall, needs_dash) {
        (0, true, true) => "ALL TOOLS".to_owned(),
        (0, true, false) => "WALL JUMP".to_owned(),
        (0, false, true) => "DASH".to_owned(),
        (coins, true, true) => format!("{coins} COINS + TOOLS"),
        (coins, true, false) => format!("{coins} COINS + WALL"),
        (coins, false, true) => format!("{coins} COINS + DASH"),
        (coins, false, false) => format!("{coins} COINS"),
    }
}

fn wall_slide_cue(
    wall_jump_enabled: bool,
    wall_sliding: bool,
    wall_contact: Option<WallSide>,
) -> Option<WallSide> {
    (wall_jump_enabled && wall_sliding)
        .then_some(wall_contact)
        .flatten()
}

fn draw_animated_player(
    viewport: &PixelViewport,
    bounds: CoreRect,
    visual: PlayerVisual,
    assets: &VisualAssets,
) {
    let logical_x = bounds.x as f32 + bounds.width as f32 / 2.0 - PLAYER_SPRITE_LOGICAL_SIZE / 2.0;
    // The generated cells include transparent foot padding. Extending the cell two logical
    // pixels below the collider aligns the visible boots with the collision floor.
    let logical_y = bounds.bottom() as f32 + 2.0 - PLAYER_SPRITE_LOGICAL_SIZE;
    draw_texture_ex(
        &assets.player_sprites,
        viewport.left + logical_x * viewport.scale,
        viewport.top + logical_y * viewport.scale,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(
                PLAYER_SPRITE_LOGICAL_SIZE * viewport.scale,
                PLAYER_SPRITE_LOGICAL_SIZE * viewport.scale,
            )),
            source: Some(visual.pose.sheet_source()),
            flip_x: visual.flip_x,
            ..Default::default()
        },
    );
}

fn draw_player_effects(
    viewport: &PixelViewport,
    bounds: CoreRect,
    wall_slide: Option<WallSide>,
    feedback: &SimulationFeedback,
    room_tick: u64,
) {
    if feedback.landing_ticks > 0 {
        let age = i32::from(LANDING_FEEDBACK_TICKS - feedback.landing_ticks);
        viewport.rectangle(
            CoreRect::new(bounds.x - 2 - age, bounds.bottom() - 1, 2, 1),
            MOVEMENT_DUST,
        );
        viewport.rectangle(
            CoreRect::new(bounds.right() + age, bounds.bottom() - 1, 2, 1),
            MOVEMENT_DUST,
        );
    }

    if feedback.skid_ticks > 0 {
        let age = i32::from(SKID_FEEDBACK_TICKS - feedback.skid_ticks);
        let behind_x = if feedback.skid_direction > 0 {
            bounds.x - 2 - age
        } else {
            bounds.right() + age
        };
        viewport.rectangle(
            CoreRect::new(behind_x, bounds.bottom() - 2, 2, 1),
            MOVEMENT_DUST,
        );
        if feedback.skid_ticks.is_multiple_of(2) {
            viewport.rectangle(
                CoreRect::new(
                    behind_x - feedback.skid_direction as i32,
                    bounds.bottom() - 4,
                    1,
                    1,
                ),
                MOVEMENT_DUST,
            );
        }
    }

    if feedback.wall_jump_ticks > 0 {
        let age = i32::from(WALL_JUMP_FEEDBACK_TICKS - feedback.wall_jump_ticks);
        let side = feedback.wall_jump_side.unwrap_or(WallSide::Right);
        let contact_x = match side {
            WallSide::Left => bounds.x - age / 2,
            WallSide::Right => bounds.right() - 1 + age / 2,
        };
        let away = match side {
            WallSide::Left => 1,
            WallSide::Right => -1,
        };
        viewport.rectangle(
            CoreRect::new(contact_x, bounds.y + 3 + age, 1, 2),
            MOVEMENT_SPARK,
        );
        viewport.rectangle(
            CoreRect::new(
                contact_x + away * (2 + age / 2),
                bounds.y + 6 + age / 2,
                2,
                1,
            ),
            WALL_SLIDE_CUE,
        );
        viewport.rectangle(
            CoreRect::new(
                contact_x + away * (1 + age / 3),
                bounds.y + 9 - age / 2,
                1,
                1,
            ),
            PLAYER_ACCENT,
        );
    } else if let Some(side) = wall_slide
        && room_tick.is_multiple_of(4)
    {
        let contact_x = match side {
            WallSide::Left => bounds.x,
            WallSide::Right => bounds.right() - 1,
        };
        viewport.rectangle(
            CoreRect::new(contact_x, bounds.bottom() - 2, 1, 1),
            WALL_SLIDE_CUE,
        );
        viewport.rectangle(
            CoreRect::new(contact_x, bounds.bottom() + 1, 1, 1),
            MOVEMENT_SPARK,
        );
    }
}

fn draw_debug_outlines(viewport: &PixelViewport, simulation: &Simulation) {
    let room = simulation.room();
    for y in 0..room.height() {
        for x in 0..room.width() {
            let tile = room.tile(x, y).expect("coordinates came from room bounds");
            let colour = match tile {
                Tile::Solid => Some(DEBUG_COLLISION),
                Tile::OneWay => Some(ONE_WAY),
                Tile::HazardUp | Tile::HazardDown | Tile::HazardLeft | Tile::HazardRight => {
                    Some(HAZARD)
                }
                Tile::Empty => None,
            };
            if let Some(colour) = colour {
                viewport.rectangle_outline(room.tile_bounds(x, y), 1, colour);
            }
        }
    }
    for exit in room.exits() {
        viewport.rectangle_outline(exit.bounds, 1, EXIT);
    }
    for door in room.doors() {
        viewport.rectangle_outline(door.trigger_bounds, 1, DOOR);
        viewport.rectangle_outline(
            CoreRect::new(
                door.arrival.x,
                door.arrival.y,
                downwards_core::PLAYER_WIDTH,
                downwards_core::PLAYER_HEIGHT,
            ),
            1,
            PLAYER_ACCENT,
        );
    }
    for hazard in room.timed_hazards() {
        viewport.rectangle_outline(hazard.bounds(), 1, HAZARD);
    }
    for pickup in room.pickups() {
        viewport.rectangle_outline(pickup.bounds(), 1, PICKUP);
    }
    viewport.rectangle_outline(simulation.player().bounds(), 1, PLAYER_ACCENT);
}

fn draw_hud(viewport: &PixelViewport, client: &ClientState) {
    // Dedicated rails sit outside the 320x180 room, keeping every ceiling and floor door visible.
    viewport.rectangle(CoreRect::new(0, 0, 320, 10), HUD_PANEL);
    let top_line = if client.human_controlled() {
        if client.is_movement_course() {
            format!(
                "A/D MOVE  JUMP  F2 TUNE  SPEED {}  BRAKE {}MS",
                client.movement_tuning.top_speed_pixels_per_second,
                client.movement_tuning.braking_milliseconds,
            )
        } else if client.selection.mode == RoomMode::CalibratedGenerated {
            "A/D MOVE  JUMP: TAP=LOW HOLD=HIGH  [ ] SEED  V WITNESS  M LEVELS".to_owned()
        } else if client.selection.mode == RoomMode::Gallery {
            "A/D MOVE  JUMP: TAP=LOW HOLD=HIGH  [ ] LEVEL  V WITNESS  M GALLERY".to_owned()
        } else if client.selection.mode.is_challenge() {
            "A/D MOVE  JUMP: TAP=LOW HOLD=HIGH  WALL: HOLD TOWARD + JUMP  R/M".to_owned()
        } else if client.selection.mode == RoomMode::Dungeon {
            let coins = client
                .dungeon_run
                .map_or(0, |run| run.inventory.coin_count());
            if !client.simulation.abilities().wall_jump {
                format!("DUNGEON · COINS {coins}/{DEMO_DUNGEON_TOTAL_COINS} · FIND CLIMBING GLOVES")
            } else if client.simulation.abilities().dash {
                format!("DUNGEON · COINS {coins}/{DEMO_DUNGEON_TOTAL_COINS} · X DASH · TAB MAP")
            } else {
                format!("DUNGEON · COINS {coins}/{DEMO_DUNGEON_TOTAL_COINS} · FIND BOOTS · TAB MAP")
            }
        } else {
            gameplay_control_summary(client.selection.tier).to_owned()
        }
    } else {
        "REPLAY OWNS INPUT  P PLAY/PAUSE  N FRAME  ESC HUMAN".to_owned()
    };
    viewport.text(&top_line, 5, 7, 6, UI_TEXT);

    viewport.rectangle(CoreRect::new(0, 190, 320, 10), HUD_PANEL);
    let room_label = match client.selection.mode {
        RoomMode::Generated => client.current_entry().map_or_else(
            || "MISSING CURATED ROOM".to_owned(),
            |entry| {
                format!(
                    "{}{}   {} {}>{}",
                    if client.catalogue.is_provisional() {
                        "PROVISIONAL / "
                    } else {
                        ""
                    },
                    client.catalogue.name(entry),
                    entry
                        .legacy()
                        .map_or("CORPUS", |legacy| catalogue_band_label(legacy.band())),
                    entry.source_door_id(),
                    entry.target_door_id()
                )
            },
        ),
        RoomMode::DeveloperGenerated => format!(
            "DEV v{}   SEED:{:X}",
            COMPOSITIONAL_GENERATION_VERSION, client.selection.seed
        ),
        RoomMode::Development => DEVELOPMENT_LEVEL_IDENTIFIER.to_owned(),
        RoomMode::Gallery => client.current_gallery_entry().map_or_else(
            || "MISSING GALLERY LEVEL".to_owned(),
            |level| {
                format!(
                    "{} {} / {}",
                    level.id().to_ascii_uppercase(),
                    level.title(),
                    level.mechanic_axis()
                )
            },
        ),
        RoomMode::CalibratedGenerated => client.current_calibrated_entry().map_or_else(
            || "MISSING CALIBRATED LEVEL".to_owned(),
            |level| {
                format!(
                    "GEN V{} SEED {:02} / {} / {}",
                    CALIBRATED_WALL_JUMP_GENERATION_VERSION,
                    level.seed(),
                    level.title(),
                    level.mechanic_axis()
                )
            },
        ),
        RoomMode::Dungeon => client.dungeon_run.map_or_else(
            || "MISSING DUNGEON RUN".to_owned(),
            |run| {
                format!(
                    "{} / COINS:{}/{} / GLOVES:{} / BOOTS:{} / CROWN:{}",
                    run.room.title(),
                    run.inventory.coin_count(),
                    DEMO_DUNGEON_TOTAL_COINS,
                    if run.inventory.climbing_gloves {
                        "ON"
                    } else {
                        "OFF"
                    },
                    if run.inventory.winged_boots {
                        "YES"
                    } else {
                        "NO"
                    },
                    if run.inventory.crown { "YES" } else { "NO" }
                )
            },
        ),
        RoomMode::DungeonV2 => client.dungeon_v2_run.as_ref().map_or_else(
            || "MISSING DUNGEON V2 RUN".to_owned(),
            |run| {
                format!(
                    "{} / COINS:{}/{} / GLOVES:{} / BOOTS:{} / CROWN:{}",
                    run.room.to_ascii_uppercase(),
                    run.inventory.coins.len(),
                    dungeon_v2_total_coins(),
                    if run.inventory.climbing_gloves {
                        "ON"
                    } else {
                        "OFF"
                    },
                    if run.inventory.winged_boots {
                        "YES"
                    } else {
                        "NO"
                    },
                    if run.inventory.crown { "YES" } else { "NO" }
                )
            },
        ),
        RoomMode::Challenge(kind) => kind.level_identifier().to_owned(),
    };
    let displayed_tier = if client.selection.mode == RoomMode::Dungeon {
        AbilityTier::from_abilities(client.simulation.abilities())
    } else {
        client.selection.tier
    };
    // The v2 dungeon rail permanently advertises the two meta keys instead of
    // the lab tier tag, keeping the room viewport itself unobstructed.
    let level_line = if client.selection.mode == RoomMode::DungeonV2 {
        format!("{room_label}   TAB MAP  ESC MENU")
    } else {
        format!(
            "{}   T{} {}",
            room_label,
            tier_number(displayed_tier),
            tier_label(displayed_tier)
        )
    };
    viewport.text(&fit_win_line(&level_line), 5, 197, 6, UI_TEXT);
}

fn draw_movement_tuning_menu(viewport: &PixelViewport, client: &ClientState) {
    let Some(menu) = client.movement_tuning_menu else {
        return;
    };
    let panel = CoreRect::new(38, 7, 244, 166);
    viewport.rectangle(panel, DEBUG_PANEL);
    viewport.rectangle_outline(panel, 1, PLAYER_ACCENT);
    viewport.centered_text("MOVEMENT LAB", 30, 10, PLAYER);
    viewport.centered_text("UP/DOWN SELECT   LEFT/RIGHT ADJUST", 41, 6, UI_DIM);

    let tuning = client.movement_tuning;
    let rows = [
        (
            "TOP SPEED",
            format!("{} px/s", tuning.top_speed_pixels_per_second),
            "maximum horizontal speed",
        ),
        (
            "ACCELERATION",
            format!("{} ms", tuning.acceleration_milliseconds),
            "ground 0-to-top; air is 1.5x",
        ),
        (
            "BRAKING",
            format!("{} ms", tuning.braking_milliseconds),
            "ground release-to-stop; air is 2x",
        ),
        (
            "WALL ASCENT",
            format!("{}%", tuning.wall_ascent_carry_percent),
            "rising wall impact becomes upward speed",
        ),
        (
            "WALL-JUMP BOOST",
            format!("{}%", tuning.wall_carry_percent),
            "reflect incoming speed into wall jump",
        ),
        (
            "WALL MEMORY",
            format!("{} ms", tuning.wall_momentum_milliseconds),
            "carry and wall-jump grace after contact",
        ),
    ];
    for (index, (label, value, detail)) in rows.iter().enumerate() {
        let y = 54 + index as i32 * 17;
        if index == menu.selected_row {
            viewport.rectangle(CoreRect::new(46, y - 9, 228, 17), MENU_SELECTED);
            viewport.text(">", 50, y + 1, 7, PLAYER_ACCENT);
        }
        viewport.text(label, 61, y, 7, PLAYER);
        viewport.text(value, 187, y, 7, PLAYER_ACCENT);
        viewport.text(detail, 61, y + 7, 5, UI_DIM);
    }
    viewport.centered_text("ENTER/F2 APPLY   R DEFAULTS   ESC CANCEL", 159, 6, UI_TEXT);
}

fn draw_death_feedback(viewport: &PixelViewport, client: &ClientState) {
    let strength = f32::from(client.feedback.death_ticks) / f32::from(DEATH_FEEDBACK_TICKS);
    viewport.rectangle(
        CoreRect::new(0, 0, 320, 180),
        Color::new(0.75, 0.015, 0.035, 0.12 * strength),
    );
    viewport.rectangle_outline(CoreRect::new(1, 1, 318, 178), 2, HAZARD);
    let panel = CoreRect::new(101, 75, 118, 30);
    viewport.rectangle(panel, DEATH_PANEL);
    viewport.rectangle_outline(panel, 1, HAZARD);
    viewport.centered_text("HIT!  INSTANT RETRY", 87, 8, PLAYER);
    viewport.centered_text(
        &format!(
            "{}  /  DEATHS {}",
            client.feedback.last_death_reason,
            client.simulation.deaths()
        ),
        98,
        6,
        UI_TEXT,
    );
}

fn draw_win_feedback(viewport: &PixelViewport, client: &ClientState) {
    let panel = CoreRect::new(40, 51, 240, 80);
    viewport.rectangle(panel, WIN_PANEL);
    viewport.rectangle_outline(panel, 1, EXIT);
    viewport.centered_text(
        if client.selection.mode == RoomMode::Dungeon {
            "THE CROWN IS YOURS"
        } else {
            "WAY DOWN OPEN"
        },
        69,
        11,
        EXIT,
    );
    let result = match client.selection.mode {
        RoomMode::Generated => client.level_name(client.selection),
        RoomMode::DeveloperGenerated => format!("DEV SEED {:X}", client.selection.seed),
        RoomMode::Development => DEVELOPMENT_LEVEL_IDENTIFIER.to_owned(),
        RoomMode::Gallery => client.level_name(client.selection),
        RoomMode::CalibratedGenerated => client.level_name(client.selection),
        RoomMode::Dungeon | RoomMode::DungeonV2 => client.level_name(client.selection),
        RoomMode::Challenge(kind) => kind.level_identifier().to_owned(),
    };
    viewport.centered_text(&result, 84, 7, UI_TEXT);
    let stats = client.stats_for(client.selection);
    let stats_line = fit_win_line(&format!(
        "RUNS {}  DEATHS {}  CLEARS {}  COINS {}  BEST {}",
        stats.attempts,
        stats.deaths,
        stats.clears,
        stats.coins_collected,
        format_best_clear(stats.best_clear_ticks)
    ));
    viewport.centered_text(&stats_line, 96, 6, PLAYER_ACCENT);
    viewport.centered_text(
        &format!(
            "T{} {}",
            tier_number(client.selection.tier),
            tier_label(client.selection.tier),
        ),
        107,
        6,
        UI_TEXT,
    );
    if client.human_controlled() {
        let next = client.next_level_selection();
        viewport.centered_text(
            &format!(
                "ENTER  NEXT: {}",
                next.map_or_else(|| "UNAVAILABLE".to_owned(), |next| client.level_name(next))
            ),
            118,
            6,
            EXIT,
        );
        viewport.centered_text("R RETRY   /   M LEVELS", 127, 6, UI_TEXT);
    } else if client.replay_complete() {
        let next = client.next_level_selection();
        viewport.centered_text(
            &format!(
                "ENTER  NEXT: {}",
                next.map_or_else(|| "UNAVAILABLE".to_owned(), |next| client.level_name(next))
            ),
            118,
            6,
            EXIT,
        );
        viewport.centered_text("P REPLAY   /   M LEVELS", 127, 6, UI_TEXT);
    } else {
        viewport.centered_text("P PAUSE   /   M LEVELS", 121, 6, UI_TEXT);
    }
}

fn draw_wrong_door_feedback(viewport: &PixelViewport, client: &ClientState) {
    let Some(reached) = client.wrong_door() else {
        return;
    };
    let target = client.selected_target_id().unwrap_or("?");
    let panel = CoreRect::new(53, 64, 214, 54);
    viewport.rectangle(panel, DEBUG_PANEL);
    viewport.rectangle_outline(panel, 1, SOURCE_DOOR);
    viewport.centered_text("THAT DOOR IS NOT THIS ROUTE", 79, 8, SOURCE_DOOR);
    viewport.centered_text(
        &format!("REACHED {reached}  /  TARGET {target}"),
        94,
        6,
        UI_TEXT,
    );
    viewport.centered_text("R RETRY FROM SOURCE   /   M LEVELS", 108, 6, PLAYER_ACCENT);
}

fn draw_replay_status(
    viewport: &PixelViewport,
    room_viewport: &PixelViewport,
    client: &ClientState,
) {
    // The status panel must never cover gameplay: everything renders as a
    // single line on the bottom HUD rail, and the detailed multi-line panel
    // appears only while a replay is deliberately paused for inspection.
    let (lines, colour) = match &client.replay_mode {
        ReplayMode::Human => {
            let Some(notice) = &client.replay_notice else {
                return;
            };
            (
                vec![notice.title.clone(), notice.detail.clone()],
                if notice.is_error {
                    HAZARD
                } else {
                    PLAYER_ACCENT
                },
            )
        }
        ReplayMode::SolveRequested(request) => {
            let (title, detail) = match request {
                SolveRequest::Route => (
                    "LOADING ROUTE WITNESS",
                    "offline-curated source-to-target replay",
                ),
                SolveRequest::Pickup(id) => ("AI SOLVING COIN", id.as_str()),
            };
            (vec![title.to_owned(), detail.to_owned()], PICKUP)
        }
        ReplayMode::Playback(playback) => {
            let phase = match playback.transport.phase {
                PlaybackPhase::Playing => "PLAYING",
                PlaybackPhase::Paused => "PAUSED",
                PlaybackPhase::Complete => "COMPLETE",
            };
            let title = |origin: &str, goal: &str| {
                format!(
                    "{} {}  {}/{}  {}",
                    origin,
                    phase,
                    playback.transport.next_frame,
                    playback.transport.total_frames,
                    goal
                )
            };
            let lines = match &playback.origin {
                ReplayOrigin::Challenge(kind) => vec![
                    title("AUTHORED", &format!("-> {}", kind.target())),
                    format!(
                        "stored exact {} challenge witness / replay verified",
                        kind.difficulty_label().to_ascii_lowercase()
                    ),
                    "wall jump enabled / dash locked".to_owned(),
                    "P PLAY/PAUSE  N FRAME  ESC HUMAN".to_owned(),
                ],
                ReplayOrigin::Gallery(level) => vec![
                    title(
                        "AUTHORED",
                        &format!("{} -> {}", level.id().to_ascii_uppercase(), level.target()),
                    ),
                    format!("{} / {}", level.title(), level.mechanic_axis()),
                    "stored tractability witness / exact replay / dash locked".to_owned(),
                    "P PLAY/PAUSE  N FRAME  ESC HUMAN".to_owned(),
                ],
                ReplayOrigin::CalibratedGenerated(level) => vec![
                    title(
                        "GENERATED",
                        &format!("SEED {:02} -> {}", level.seed(), level.target()),
                    ),
                    format!("{} / {}", level.title(), level.mechanic_axis()),
                    "mechanically simplified witness / exact replay / dash locked".to_owned(),
                    "P PLAY/PAUSE  N FRAME  ESC HUMAN".to_owned(),
                ],
                ReplayOrigin::Solver(diagnostics) => {
                    let goal = playback
                        .expected_objective
                        .as_ref()
                        .map_or_else(|| "WITNESS".to_owned(), ReplayObjective::status_label);
                    vec![
                        title("AI", &goal),
                        format!(
                            "PROVISIONAL {}  TEMP ROBUST {}",
                            complexity_band_label(diagnostics.provisional_band),
                            format_robustness(diagnostics.temporal_robustness)
                        ),
                        format!(
                            "accepted wall-jump {}  dash {}",
                            diagnostics.accepted_wall_jumps, diagnostics.accepted_dashes
                        ),
                        format!(
                            "nodes {}  sim ticks {}",
                            diagnostics.search_stats.expanded_nodes,
                            diagnostics.search_stats.simulated_ticks
                        ),
                        "P PLAY/PAUSE  N FRAME  ESC HUMAN".to_owned(),
                    ]
                }
                ReplayOrigin::CatalogueRoute {
                    band,
                    stored_witness,
                    successful_wall_jumps,
                    successful_dashes,
                    search_stats,
                } => {
                    let goal = playback
                        .expected_objective
                        .as_ref()
                        .map_or_else(|| "DOOR".to_owned(), ReplayObjective::status_label);
                    let mut lines = vec![
                        title(if *stored_witness { "CURATED" } else { "AI" }, &goal),
                        format!(
                            "ASSESSED {}  WALL-JUMPS {}  DASHES {}",
                            band.map_or("CORPUS", catalogue_band_label),
                            successful_wall_jumps,
                            successful_dashes
                        ),
                    ];
                    if let Some(search_stats) = search_stats {
                        lines.push(format!(
                            "fallback nodes {}  sim ticks {}",
                            search_stats.expanded_nodes, search_stats.simulated_ticks
                        ));
                    } else {
                        lines.push("stored offline witness / exact replay verified".to_owned());
                    }
                    lines.push("P PLAY/PAUSE  N FRAME  ESC HUMAN".to_owned());
                    lines
                }
                ReplayOrigin::PickupSolver {
                    pickup_id,
                    stored_witness,
                    search_stats,
                } => {
                    let mut lines = vec![
                        title(
                            if *stored_witness {
                                "CURATED COIN"
                            } else {
                                "COIN AI"
                            },
                            &format!("COLLECT {pickup_id}"),
                        ),
                        "same source door / exact pickup replay verified".to_owned(),
                    ];
                    if let Some(search_stats) = search_stats {
                        lines.push(format!(
                            "nodes {}  sim ticks {}",
                            search_stats.expanded_nodes, search_stats.simulated_ticks
                        ));
                    }
                    lines.push("P PLAY/PAUSE  N FRAME  ESC HUMAN".to_owned());
                    lines
                }
                ReplayOrigin::Human(outcome) => vec![
                    title("HUMAN", outcome.label()),
                    format!("recorded {} semantic frames", playback.replay.frames.len()),
                    "P PLAY/PAUSE  N FRAME  ESC HUMAN".to_owned(),
                ],
            };
            (
                lines,
                if playback.transport.phase == PlaybackPhase::Complete {
                    EXIT
                } else {
                    PLAYER_ACCENT
                },
            )
        }
    };
    let mut rail_line = lines.first().cloned().unwrap_or_default();
    if let Some(detail) = lines.get(1)
        && !detail.is_empty()
    {
        rail_line = format!("{rail_line} \u{00b7} {detail}");
    }
    viewport.rectangle(CoreRect::new(0, 190, 320, 10), HUD_PANEL);
    viewport.text(&fit_status_line(&rail_line), 5, 197, 6, colour);

    if client.replay_paused() {
        let panel_height = 5 + lines.len() as i32 * 8;
        room_viewport.rectangle(CoreRect::new(4, 12, 150, panel_height), DEBUG_PANEL);
        room_viewport.rectangle_outline(CoreRect::new(4, 12, 150, panel_height), 1, colour);
        for (index, line) in lines.iter().enumerate() {
            let line_colour = if index == 0 { colour } else { UI_TEXT };
            room_viewport.text(
                &fit_status_line(line),
                8,
                20 + index as i32 * 8,
                6,
                line_colour,
            );
        }
    }
}

fn fit_status_line(text: &str) -> String {
    const MAX_CHARS: usize = 47;
    if text.chars().count() <= MAX_CHARS {
        return text.to_owned();
    }
    let mut fitted: String = text.chars().take(MAX_CHARS - 3).collect();
    fitted.push_str("...");
    fitted
}

fn fit_preview_line(text: &str) -> String {
    const MAX_CHARS: usize = 54;
    if text.chars().count() <= MAX_CHARS {
        return text.to_owned();
    }
    let mut fitted: String = text.chars().take(MAX_CHARS - 3).collect();
    fitted.push_str("...");
    fitted
}

fn fit_win_line(text: &str) -> String {
    const MAX_CHARS: usize = 80;
    if text.chars().count() <= MAX_CHARS {
        return text.to_owned();
    }
    let mut fitted: String = text.chars().take(MAX_CHARS - 3).collect();
    fitted.push_str("...");
    fitted
}

fn format_best_clear(ticks: Option<usize>) -> String {
    ticks.map_or_else(
        || "--".to_owned(),
        |ticks| {
            format!(
                "{:.2}s/{}t",
                ticks as f64 / f64::from(TICKS_PER_SECOND),
                ticks
            )
        },
    )
}

fn draw_debug_overlay(viewport: &PixelViewport, client: &ClientState) {
    let simulation = &client.simulation;
    let player = simulation.player();
    let abilities = simulation.abilities();
    let position = player.position_subpixels();
    let velocity = player.velocity_subpixels();
    let wall = match player.wall_contact() {
        Some(WallSide::Left) => "L",
        Some(WallSide::Right) => "R",
        None => "-",
    };
    let dash = if !abilities.dash {
        "LOCKED"
    } else if player.dash_available() {
        "READY"
    } else {
        "SPENT"
    };

    let provenance = match client.generated_provenance {
        Some(provenance) if provenance.corpus_generator.is_some() => format!(
            "corpus {} / native key",
            provenance.corpus_generator.expect("checked above")
        ),
        Some(provenance) => format!(
            "gen v{}  {} / {}",
            provenance.generation_version,
            provenance.strategy.slug(),
            provenance.intent.slug()
        ),
        None if client.selection.mode == RoomMode::Gallery => {
            client.current_gallery_entry().map_or_else(
                || "authored gallery / missing registration".to_owned(),
                |level| {
                    format!(
                        "authored gallery {} / exact witness / dash locked",
                        level.id()
                    )
                },
            )
        }
        None if client.selection.mode.is_challenge() => {
            let kind = client
                .selection
                .mode
                .challenge()
                .expect("challenge mode checked above");
            format!(
                "authored {} challenge / exact witness / dash locked",
                kind.difficulty_label().to_ascii_lowercase()
            )
        }
        None => "gen --  DEV: FIRST STEPS".to_owned(),
    };
    let mut lines = vec![
        format!(
            "{}  T{} {}  W{} D{}",
            room_mode_label(client.selection.mode),
            tier_number(client.selection.tier),
            tier_label(client.selection.tier),
            u8::from(abilities.wall_jump),
            u8::from(abilities.dash)
        ),
        format!(
            "seed {:016x}",
            client
                .generated_provenance
                .map_or(client.selection.seed, |provenance| provenance.seed)
        ),
        provenance,
        format!(
            "tick {}  room {}",
            simulation.tick(),
            simulation.room_tick()
        ),
        format!(
            "pos {}  {} px",
            format_subpixels(position.x),
            format_subpixels(position.y)
        ),
        format!(
            "vel {}  {} px/t",
            format_subpixels(velocity.x),
            format_subpixels(velocity.y)
        ),
        format!(
            "ground {}  wall {}  slide {}",
            yes_no(player.grounded()),
            wall,
            yes_no(player.wall_sliding())
        ),
        format!(
            "jump tap window {}ms  boost {}  input v{}",
            HUMAN_JUMP_TAP_WINDOW_MICROS / 1_000,
            player.jump_hold_ticks_remaining(),
            HUMAN_JUMP_INPUT_POLICY_VERSION
        ),
        format!(
            "movement {} / F2 tunes",
            movement_tuning_summary(simulation.movement_tuning())
        ),
        format!(
            "dash {}  ticks {}  drop {}",
            dash,
            player.dash_ticks_remaining(),
            player.one_way_drop_ticks_remaining()
        ),
        format!("deaths {}", simulation.deaths()),
        format!("digest {}", simulation.digest()),
    ];
    if let ReplayMode::Playback(playback) = &client.replay_mode
        && let ReplayOrigin::Solver(diagnostics) = &playback.origin
    {
        lines.push(format!(
            "PROVISIONAL {} temp robust {}",
            complexity_band_label(diagnostics.provisional_band),
            format_robustness(diagnostics.temporal_robustness)
        ));
        lines.push(format!(
            "accepted WJ {} DASH {}",
            diagnostics.accepted_wall_jumps, diagnostics.accepted_dashes
        ));
    }
    let panel_height = 5 + lines.len() as i32 * 8;
    viewport.rectangle(CoreRect::new(158, 12, 158, panel_height), DEBUG_PANEL);
    viewport.rectangle_outline(CoreRect::new(158, 12, 158, panel_height), 1, UI_DIM);
    for (index, line) in lines.iter().enumerate() {
        viewport.text(line, 162, 20 + index as i32 * 8, 6, UI_TEXT);
    }
}

fn complexity_band_label(band: ComplexityBand) -> &'static str {
    match band {
        ComplexityBand::Gentle => "GENTLE",
        ComplexityBand::Standard => "STANDARD",
        ComplexityBand::Technical => "TECHNICAL",
    }
}

fn format_robustness(ratio: Option<f64>) -> String {
    ratio.map_or_else(
        || "N/A".to_owned(),
        |ratio| format!("{:.0}%", ratio * 100.0),
    )
}

fn yes_no(value: bool) -> &'static str {
    if value { "Y" } else { "N" }
}

fn format_subpixels(value: i32) -> String {
    let tenths = i64::from(value) * 10 / i64::from(SUBPIXELS_PER_PIXEL);
    let sign = if tenths < 0 { "-" } else { "" };
    let magnitude = tenths.abs();
    format!("{sign}{}.{:01}", magnitude / 10, magnitude % 10)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PixelViewport {
    scale: f32,
    left: f32,
    top: f32,
}

impl PixelViewport {
    fn for_window(width: f32, height: f32) -> Self {
        let available_scale = (width / LOGICAL_WIDTH).min(height / LOGICAL_HEIGHT);
        // Normal windows use an integer scale. Falling back to a fractional scale only keeps the
        // game visible if a window manager makes the window smaller than its logical canvas.
        let scale = if available_scale >= 1.0 {
            available_scale.floor()
        } else {
            available_scale.max(0.01)
        };
        let left = ((width - LOGICAL_WIDTH * scale) / 2.0).floor();
        let top = ((height - LOGICAL_HEIGHT * scale) / 2.0).floor();
        Self { scale, left, top }
    }

    fn fill_logical_screen(&self, colour: Color) {
        draw_rectangle(
            self.left,
            self.top,
            LOGICAL_WIDTH * self.scale,
            LOGICAL_HEIGHT * self.scale,
            colour,
        );
    }

    fn translated(&self, logical_x: i32, logical_y: i32) -> Self {
        Self {
            scale: self.scale,
            left: self.screen_x(logical_x),
            top: self.screen_y(logical_y),
        }
    }

    fn rectangle(&self, bounds: CoreRect, colour: Color) {
        draw_rectangle(
            self.screen_x(bounds.x),
            self.screen_y(bounds.y),
            bounds.width as f32 * self.scale,
            bounds.height as f32 * self.scale,
            colour,
        );
    }

    fn rectangle_outline(&self, bounds: CoreRect, thickness: i32, colour: Color) {
        draw_rectangle_lines(
            self.screen_x(bounds.x),
            self.screen_y(bounds.y),
            bounds.width as f32 * self.scale,
            bounds.height as f32 * self.scale,
            thickness as f32 * self.scale,
            colour,
        );
    }

    fn triangle(&self, first: (i32, i32), second: (i32, i32), third: (i32, i32), colour: Color) {
        draw_triangle(
            vec2(self.screen_x(first.0), self.screen_y(first.1)),
            vec2(self.screen_x(second.0), self.screen_y(second.1)),
            vec2(self.screen_x(third.0), self.screen_y(third.1)),
            colour,
        );
    }

    fn text(&self, text: &str, x: i32, baseline_y: i32, size: u16, colour: Color) {
        draw_text(
            text,
            self.screen_x(x),
            self.screen_y(baseline_y),
            f32::from(size) * self.scale,
            colour,
        );
    }

    fn centered_text(&self, text: &str, baseline_y: i32, size: u16, colour: Color) {
        let scaled_size = (f32::from(size) * self.scale).round() as u16;
        let metrics = measure_text(text, None, scaled_size, 1.0);
        let x = self.left + (LOGICAL_WIDTH * self.scale - metrics.width) / 2.0;
        draw_text(
            text,
            x.floor(),
            self.screen_y(baseline_y),
            f32::from(scaled_size),
            colour,
        );
    }

    fn centered_text_in(
        &self,
        bounds: CoreRect,
        text: &str,
        baseline_y: i32,
        size: u16,
        colour: Color,
    ) {
        let scaled_size = (f32::from(size) * self.scale).round() as u16;
        let metrics = measure_text(text, None, scaled_size, 1.0);
        let x = self.screen_x(bounds.x) + (bounds.width as f32 * self.scale - metrics.width) / 2.0;
        draw_text(
            text,
            x.floor(),
            self.screen_y(baseline_y),
            f32::from(scaled_size),
            colour,
        );
    }

    fn screen_x(&self, logical_x: i32) -> f32 {
        self.left + logical_x as f32 * self.scale
    }

    fn screen_y(&self, logical_y: i32) -> f32 {
        self.top + logical_y as f32 * self.scale
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use downwards_core::{Pickup, Point};

    use super::*;

    fn temporary_test_path(label: &str, file_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock follows Unix epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("downwards-{label}-{}-{nonce}", std::process::id()))
            .join(file_name)
    }

    #[test]
    fn command_line_defaults_to_the_dungeon_v2_title_screen() {
        let options = parse_launch_options(Vec::<String>::new()).unwrap();
        assert!(options.title_screen);
        assert_eq!(options.selection.mode, RoomMode::DungeonV2);
        assert_eq!(options.selection.tier, AbilityTier::Baseline);
        assert!(!options.show_help);
        assert_eq!(
            options.history_path,
            PathBuf::from(DEFAULT_HUMAN_HISTORY_PATH)
        );
        assert_eq!(
            options.dungeon_save_path,
            PathBuf::from(DEFAULT_DUNGEON_V2_SAVE_PATH)
        );
        assert!(!options.fresh_dungeon);
    }

    #[test]
    fn explicit_lab_and_dungeon_flags_skip_the_title_screen() {
        for arguments in [
            vec!["--generated"],
            vec!["--development"],
            vec!["--seed", "9"],
            vec!["--tier", "2"],
            vec!["--gallery"],
            vec!["--calibrated"],
            vec!["--challenge"],
            vec!["--dungeon"],
            vec!["--dungeon-v2"],
            vec!["--dungeon-v2-new"],
            vec!["--dungeon-floor", "1"],
        ] {
            let options = parse_launch_options(arguments.clone()).unwrap();
            assert!(!options.title_screen, "{arguments:?} must skip the title");
        }
        assert_eq!(
            parse_launch_options(["--generated"]).unwrap().selection,
            ScenarioSelection::default()
        );
    }

    #[test]
    fn title_menu_hides_continue_without_a_save() {
        let menu = TitleMenuState::new(false);
        assert_eq!(
            menu.items(),
            &[
                TitleMenuItem::NewGame,
                TitleMenuItem::Controls,
                TitleMenuItem::Quit
            ]
        );
        let menu = TitleMenuState::new(true);
        assert_eq!(menu.items()[0], TitleMenuItem::Continue);
        assert_eq!(menu.selected_item(), TitleMenuItem::Continue);
    }

    #[test]
    fn title_menu_navigation_wraps_and_activates() {
        let mut menu = TitleMenuState::new(true);
        menu.move_up();
        assert_eq!(menu.selected_item(), TitleMenuItem::Quit);
        assert_eq!(menu.activate(), Some(TitleAction::Quit));
        menu.move_down();
        assert_eq!(menu.selected_item(), TitleMenuItem::Continue);
        assert_eq!(menu.activate(), Some(TitleAction::Continue));
        menu.move_down();
        menu.move_down();
        assert_eq!(menu.selected_item(), TitleMenuItem::Controls);
        assert_eq!(menu.activate(), Some(TitleAction::ShowControls));
    }

    #[test]
    fn title_new_game_requires_confirmation_only_over_an_existing_save() {
        let mut fresh = TitleMenuState::new(false);
        assert_eq!(fresh.selected_item(), TitleMenuItem::NewGame);
        assert_eq!(fresh.activate(), Some(TitleAction::NewGame));

        let mut menu = TitleMenuState::new(true);
        menu.move_down();
        assert_eq!(menu.selected_item(), TitleMenuItem::NewGame);
        assert_eq!(menu.activate(), None);
        assert!(menu.confirm_new_game);
        assert_eq!(menu.activate(), Some(TitleAction::NewGame));

        // Navigating away withdraws the pending confirmation.
        let mut menu = TitleMenuState::new(true);
        menu.move_down();
        assert_eq!(menu.activate(), None);
        menu.move_down();
        menu.move_up();
        assert_eq!(menu.selected_item(), TitleMenuItem::NewGame);
        assert_eq!(menu.activate(), None);
    }

    #[test]
    fn dungeon_v2_save_playability_gates_continue() {
        let path = temporary_test_path("title-save", "dungeon-v2-save-v1.json");
        assert!(!dungeon_v2_save_playable(&path));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not json").unwrap();
        assert!(!dungeon_v2_save_playable(&path));
        let save = DungeonV2SaveV1::from_run(
            &DungeonV2RunState::default(),
            None,
            &BTreeSet::from([dungeon_v2_definition().spawn.clone()]),
        );
        fs::write(&path, serde_json::to_vec(&save).unwrap()).unwrap();
        assert!(dungeon_v2_save_playable(&path));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn command_line_parses_decimal_hex_named_tiers_and_mode() {
        let decimal = parse_launch_options(["--seed", "42", "--tier", "3"]).unwrap();
        assert_eq!(decimal.selection.seed, 42);
        assert_eq!(decimal.selection.tier, AbilityTier::Dash);

        let hexadecimal =
            parse_launch_options(["--seed=0xfeed_cafe", "--tier=wall-jump", "--development"])
                .unwrap();
        assert_eq!(hexadecimal.selection.seed, 0xfeed_cafe);
        assert_eq!(hexadecimal.selection.tier, AbilityTier::WallJump);
        assert_eq!(hexadecimal.selection.mode, RoomMode::Development);
    }

    #[test]
    fn command_line_rejects_missing_and_invalid_values() {
        assert_eq!(
            parse_launch_options(["--seed"]).unwrap_err(),
            "--seed needs a value"
        );
        assert!(parse_launch_options(["--seed", "-1"]).is_err());
        assert!(parse_launch_options(["--history"]).is_err());
        assert!(parse_launch_options(["--history="]).is_err());
        assert!(parse_launch_options(["--dungeon-save"]).is_err());
        assert!(parse_launch_options(["--dungeon-save="]).is_err());
        assert!(parse_launch_options(["--dungeon-save", "save.json"]).is_err());
        assert!(parse_launch_options(["--tier", "5"]).is_err());
        assert!(parse_launch_options(["42"]).is_err());
    }

    #[test]
    fn human_history_path_is_explicit_and_works_with_gallery_mode() {
        let options = parse_launch_options(["--gallery", "--history", "feedback/run.jsonl"])
            .expect("history path should be independent of the selected mode");
        assert_eq!(options.selection.mode, RoomMode::Gallery);
        assert_eq!(options.history_path, PathBuf::from("feedback/run.jsonl"));
    }

    #[test]
    fn raw_seed_is_an_explicit_developer_mode() {
        let options = parse_launch_options(["--seed", "0xffff", "--tier", "dash"]).unwrap();
        assert_eq!(options.selection.mode, RoomMode::DeveloperGenerated);
        assert_eq!(options.selection.seed, 0xffff);
        assert_eq!(options.selection.tier, AbilityTier::Dash);
    }

    #[test]
    fn corpus_manifest_is_explicit_and_provisional_opt_in_is_scoped() {
        let options =
            parse_launch_options(["--corpus", "playtest.manifest", "--tier", "dash"]).unwrap();
        assert_eq!(
            options.corpus_manifest.as_deref(),
            Some("playtest.manifest")
        );
        assert_eq!(options.selection.tier, AbilityTier::Dash);
        assert!(!options.allow_provisional_corpus);
        assert!(parse_launch_options(["--allow-provisional-corpus"]).is_err());
        assert!(parse_launch_options(["--corpus=x", "--seed", "4"]).is_err());
    }

    #[test]
    fn challenge_launch_is_explicit_locked_and_not_a_corpus_mode() {
        let options = parse_launch_options(["--challenge"]).unwrap();
        assert_eq!(
            options.selection.mode,
            RoomMode::Challenge(ChallengeKind::Hard)
        );
        assert_eq!(options.selection.seed, 0);
        assert_eq!(options.selection.tier, AbilityTier::WallJump);
        assert_eq!(options.corpus_manifest, None);
        assert!(!options.allow_provisional_corpus);

        assert_eq!(
            parse_launch_options(["--challenge", "hard"])
                .unwrap()
                .selection
                .mode,
            RoomMode::Challenge(ChallengeKind::Hard)
        );
        assert_eq!(
            parse_launch_options(["--challenge", "tutorial"])
                .unwrap()
                .selection
                .mode,
            RoomMode::Challenge(ChallengeKind::Medium)
        );
        assert_eq!(
            parse_launch_options(["--challenge", "medium"])
                .unwrap()
                .selection
                .mode,
            RoomMode::Challenge(ChallengeKind::Medium)
        );
        assert_eq!(
            parse_launch_options(["--challenge=medium"])
                .unwrap()
                .selection
                .mode,
            RoomMode::Challenge(ChallengeKind::Medium)
        );

        assert!(parse_launch_options(["--challenge", "unknown"]).is_err());
        assert!(parse_launch_options(["--challenge="]).is_err());
        assert!(parse_launch_options(["--challenge", "hard", "--challenge=medium"]).is_err());
        assert!(parse_launch_options(["--challenge", "--tier", "wall-jump"]).is_err());
        assert!(parse_launch_options(["--challenge", "--seed", "7"]).is_err());
        assert!(parse_launch_options(["--development", "--challenge"]).is_err());
        assert!(parse_launch_options(["--challenge", "--corpus=x"]).is_err());
    }

    #[test]
    fn dungeon_launch_is_explicit_and_uses_its_persistent_content_loadout() {
        let options = parse_launch_options(["--dungeon"]).unwrap();
        assert_eq!(options.selection.mode, RoomMode::Dungeon);
        assert_eq!(options.selection.seed, 0);
        assert_eq!(options.selection.tier, AbilityTier::Baseline);
        assert!(!options.fresh_dungeon);
        let fresh =
            parse_launch_options(["--dungeon-new", "--dungeon-save", "feedback/dungeon.json"])
                .unwrap();
        assert_eq!(fresh.selection.mode, RoomMode::Dungeon);
        assert!(fresh.fresh_dungeon);
        assert_eq!(
            fresh.dungeon_save_path,
            PathBuf::from("feedback/dungeon.json")
        );
        assert!(parse_launch_options(["--dungeon", "--tier", "dash"]).is_err());
        assert!(parse_launch_options(["--dungeon", "--gallery"]).is_err());
        assert!(parse_launch_options(["--dungeon", "--seed", "3"]).is_err());
        assert!(parse_launch_options(["--dungeon", "--dungeon-new"]).is_err());
    }

    #[test]
    fn dungeon_floor_lab_accepts_numbers_ids_and_titles_without_persistence() {
        let first = parse_launch_options(["--dungeon-floor", "1"]).unwrap();
        assert_eq!(
            first.selection.mode,
            RoomMode::Challenge(ChallengeKind::DungeonFloor(DemoDungeonRoom::HollowLanding))
        );
        assert_eq!(first.selection.tier, AbilityTier::Baseline);
        assert!(!first.fresh_dungeon);

        for value in ["gale-chasm", "demo-dungeon.gale-chasm", "Gale Chasm"] {
            assert_eq!(
                parse_launch_options(["--dungeon-floor", value])
                    .unwrap()
                    .selection
                    .mode,
                RoomMode::Challenge(ChallengeKind::DungeonFloor(DemoDungeonRoom::DashChasm))
            );
        }
        let last = parse_launch_options(["--dungeon-floor=101"]).unwrap();
        assert_eq!(
            last.selection.mode,
            RoomMode::Challenge(ChallengeKind::DungeonFloor(DemoDungeonRoom::CrownSanctum))
        );
        assert_eq!(last.selection.tier, AbilityTier::WallJumpAndDash);

        assert!(parse_launch_options(["--dungeon-floor", "0"]).is_err());
        assert!(parse_launch_options(["--dungeon-floor", "102"]).is_err());
        assert!(parse_launch_options(["--dungeon-floor", "missing"]).is_err());
        assert!(parse_launch_options(["--dungeon-floor"]).is_err());
        assert!(parse_launch_options(["--dungeon-floor", "1", "--dungeon"]).is_err());
        assert!(parse_launch_options(["--dungeon-floor", "1", "--tier", "dash"]).is_err());
        assert!(
            parse_launch_options(["--dungeon-floor", "1", "--dungeon-save", "save.json"]).is_err()
        );
    }

    #[test]
    fn dungeon_save_roundtrips_exact_progress_and_rejects_corruption() {
        let path = temporary_test_path("dungeon-save-roundtrip", "save.json");
        let (persistence, loaded) = DungeonPersistence::open(&path, false).unwrap();
        assert!(loaded.is_none());
        let mut inventory = DemoDungeonInventory::default();
        inventory.climbing_gloves = true;
        assert!(inventory.collect_coin("dungeon-coin-00"));
        assert!(inventory.collect_coin("dungeon-coin-17"));
        let expected = DungeonRunState {
            room: DemoDungeonRoom::BroadChimney,
            inventory,
        };
        let explored = BTreeSet::from([
            DemoDungeonRoom::HollowLanding,
            DemoDungeonRoom::BroadChimney,
        ]);
        persistence
            .persist(expected, Some("west"), &explored)
            .unwrap();

        let (_, loaded) = DungeonPersistence::open(&path, false).unwrap();
        assert_eq!(
            loaded,
            Some((expected, Some("west".to_owned()), explored.clone()))
        );
        let mut save: DungeonSaveV1 = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        save.collected_coins = vec![0, 0];
        assert!(save.validate().unwrap_err().contains("unique, sorted"));
        save.collected_coins = vec![0, 17];
        save.palette_generation_version += 1;
        assert!(
            save.validate()
                .unwrap_err()
                .contains("different authored dungeon version")
        );
        fs::write(&path, b"{}\n").unwrap();
        assert!(
            DungeonPersistence::open(&path, false)
                .unwrap_err()
                .contains("parse")
        );
    }

    #[test]
    fn fresh_dungeon_archives_the_previous_save_instead_of_erasing_it() {
        let path = temporary_test_path("dungeon-save-fresh", "save.json");
        let (persistence, _) = DungeonPersistence::open(&path, false).unwrap();
        persistence
            .persist(
                DungeonRunState::default(),
                None,
                &BTreeSet::from([DemoDungeonRoom::HollowLanding]),
            )
            .unwrap();
        let original = fs::read(&path).unwrap();

        let (_, loaded) = DungeonPersistence::open(&path, true).unwrap();
        assert!(loaded.is_none());
        assert!(!path.exists());
        let archives = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".bak"))
            })
            .collect::<Vec<_>>();
        assert_eq!(archives.len(), 1);
        assert_eq!(fs::read(&archives[0]).unwrap(), original);
    }

    use downwards_content::dungeon_v2_coin_id;

    #[test]
    fn dungeon_v2_client_transitions_gates_and_persists() {
        let save_path = temporary_test_path("dungeon-v2-client", "save.json");
        let selection = ScenarioSelection {
            mode: RoomMode::DungeonV2,
            seed: 0,
            tier: AbilityTier::Baseline,
        };
        let mut client = ClientState::new_with_persistence(
            selection,
            None,
            false,
            None,
            Some((&save_path, false)),
        )
        .unwrap();
        let spawn = dungeon_v2_definition().spawn.clone();
        assert_eq!(
            client.dungeon_v2_run.as_ref().map(|run| run.room.clone()),
            Some(spawn.clone())
        );
        let report = |client: &ClientState, events: Vec<SimulationEvent>| StepReport {
            tick: client.simulation.tick(),
            events,
            digest: client.simulation.digest(),
        };

        // A banked coin persists into the save.
        let coin = dungeon_v2_coin_id(&spawn, 0);
        assert!(client.observe_dungeon_v2_progress(&report(
            &client,
            vec![SimulationEvent::PickupCollected { id: coin.clone() }],
        )));
        assert!(
            client
                .dungeon_v2_run
                .as_ref()
                .unwrap()
                .inventory
                .coins
                .contains(&coin)
        );

        // A sealed door bounces an unequipped player; meeting its requirement
        // opens it. The gated door is discovered from the live layout so
        // redesigns keep the test honest.
        let dungeon = dungeon_v2_definition();
        let (gated_room, gated_door, requirement) = dungeon
            .instances
            .iter()
            .find_map(|(instance_id, instance)| {
                instance.connections.keys().find_map(|door_id| {
                    dungeon_v2_door_requirement(instance_id, door_id)
                        .map(|requirement| (instance_id.clone(), door_id.clone(), requirement))
                })
            })
            .expect("the layout seals at least one door");
        let entry_door = dungeon.instances[&gated_room]
            .connections
            .keys()
            .find(|door_id| **door_id != gated_door)
            .unwrap_or(&gated_door)
            .clone();
        client.dungeon_v2_run.as_mut().unwrap().room = gated_room.clone();
        let room = dungeon_v2_room(
            &gated_room,
            &client.dungeon_v2_run.as_ref().unwrap().inventory,
        );
        client.simulation =
            Simulation::enter_via_door(room, AbilitySet::NONE, &entry_door).unwrap();
        assert!(client.observe_dungeon_v2_progress(&report(
            &client,
            vec![SimulationEvent::ExitReached {
                id: gated_door.clone(),
            }],
        )));
        assert_eq!(
            client.dungeon_v2_run.as_ref().unwrap().room,
            gated_room,
            "a sealed door must not transition"
        );
        assert!(client.replay_notice.as_ref().unwrap().is_error);

        // Satisfy the requirement, then the same door transitions.
        {
            let run = client.dungeon_v2_run.as_mut().unwrap();
            match requirement {
                DungeonV2Requirement::ClimbingGloves => run.inventory.climbing_gloves = true,
                DungeonV2Requirement::WingedBoots => run.inventory.winged_boots = true,
                DungeonV2Requirement::Coins(count) => {
                    for index in 0..count {
                        run.inventory.coins.insert(format!("v2-coin-test-{index}"));
                    }
                }
            }
            assert!(run.inventory.satisfies(requirement));
        }
        assert!(client.observe_dungeon_v2_progress(&report(
            &client,
            vec![SimulationEvent::ExitReached { id: gated_door }],
        )));
        let destination = client.dungeon_v2_run.as_ref().unwrap().room.clone();
        assert_ne!(destination, gated_room);
        assert!(client.explored_v2_rooms.contains(&destination));

        // The save round-trips the full run state.
        let saved: DungeonV2SaveV1 =
            serde_json::from_slice(&fs::read(&save_path).unwrap()).unwrap();
        let (run, _, explored) = saved.validate().unwrap();
        assert_eq!(run, *client.dungeon_v2_run.as_ref().unwrap());
        assert_eq!(explored, client.explored_v2_rooms);
    }

    #[test]
    fn crown_goal_exit_triggers_the_victory_screen_and_keeps_the_save() {
        let save_path = temporary_test_path("dungeon-v2-victory", "save.json");
        let selection = ScenarioSelection {
            mode: RoomMode::DungeonV2,
            seed: 0,
            tier: AbilityTier::Baseline,
        };
        let mut client = ClientState::new_with_persistence(
            selection,
            None,
            false,
            None,
            Some((&save_path, false)),
        )
        .unwrap();
        let report = |client: &ClientState, events: Vec<SimulationEvent>| StepReport {
            tick: client.simulation.tick(),
            events,
            digest: client.simulation.digest(),
        };

        // Without the crown, the terminal exit does nothing.
        assert!(client.observe_dungeon_v2_progress(&report(
            &client,
            vec![SimulationEvent::ExitReached {
                id: DEMO_DUNGEON_GOAL_EXIT.to_owned(),
            }],
        )));
        assert!(!client.dungeon_v2_victory);

        client.dungeon_v2_run.as_mut().unwrap().inventory.crown = true;
        assert!(client.observe_dungeon_v2_progress(&report(
            &client,
            vec![SimulationEvent::ExitReached {
                id: DEMO_DUNGEON_GOAL_EXIT.to_owned(),
            }],
        )));
        assert!(client.dungeon_v2_victory);

        // The finished run stays on disk as continuable history.
        assert!(dungeon_v2_save_playable(&save_path));
        fs::remove_dir_all(save_path.parent().unwrap()).unwrap();
    }

    #[test]
    fn quit_to_title_state_resets_when_a_new_session_starts() {
        let save_path = temporary_test_path("dungeon-v2-quit", "save.json");
        let selection = ScenarioSelection {
            mode: RoomMode::DungeonV2,
            seed: 0,
            tier: AbilityTier::Baseline,
        };
        let mut client = ClientState::new_with_persistence(
            selection,
            None,
            false,
            None,
            Some((&save_path, false)),
        )
        .unwrap();
        client.pause_visible = true;
        client.pause_menu_index = PAUSE_MENU_ITEMS.len() - 1;
        client.controls_visible = true;
        client.quit_to_title = true;
        drop(client);

        // Continuing from the title rebuilds a clean session over the same save.
        let resumed = ClientState::new_with_persistence(
            selection,
            None,
            false,
            None,
            Some((&save_path, false)),
        )
        .unwrap();
        assert!(!resumed.quit_to_title);
        assert!(!resumed.pause_visible);
        assert!(!resumed.controls_visible);
        assert!(!resumed.dungeon_v2_victory);
        assert_eq!(resumed.pause_menu_index, 0);
        assert!(resumed.dungeon_v2_run.is_some());
        fs::remove_dir_all(save_path.parent().unwrap()).unwrap();
    }

    #[test]
    fn dungeon_client_resumes_room_inventory_and_logs_progress_events() {
        let save_path = temporary_test_path("dungeon-client-resume", "save.json");
        let history_path = save_path.parent().unwrap().join("history.jsonl");
        let selection = ScenarioSelection {
            mode: RoomMode::Dungeon,
            seed: 0,
            tier: AbilityTier::Baseline,
        };
        let mut client = ClientState::new_with_persistence(
            selection,
            None,
            false,
            Some(&history_path),
            Some((&save_path, false)),
        )
        .unwrap();
        let report = |client: &ClientState, events: Vec<SimulationEvent>| StepReport {
            tick: client.simulation.tick(),
            events,
            digest: client.simulation.digest(),
        };
        assert!(!client.observe_dungeon_progress(&report(
            &client,
            vec![SimulationEvent::PickupCollected {
                id: "dungeon-coin-00".to_owned(),
            }],
        )));
        assert!(client.observe_dungeon_progress(&report(
            &client,
            vec![SimulationEvent::ExitReached {
                id: "east".to_owned(),
            }],
        )));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::MossWalk);
        drop(client);

        let resumed = ClientState::new_with_persistence(
            selection,
            None,
            false,
            None,
            Some((&save_path, false)),
        )
        .unwrap();
        let run = resumed.dungeon_run.unwrap();
        assert_eq!(run.room, DemoDungeonRoom::MossWalk);
        assert!(run.inventory.has_coin("dungeon-coin-00"));
        assert_eq!(resumed.simulation.entry_door(), Some("west"));
        assert!(
            resumed
                .simulation
                .room()
                .pickups()
                .iter()
                .all(|pickup| pickup.id() != "dungeon-coin-00")
        );

        let progress = fs::read_to_string(&history_path).unwrap();
        let events = progress
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|record| record["schema"] == "downwards-dungeon-progress-v2")
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]["dungeon_definition_id"],
            demo_dungeon_definition().id
        );
        assert_eq!(
            events[0]["palette_generation_version"],
            DUNGEON_PALETTE_GENERATION_VERSION
        );
        assert_eq!(events[0]["event"], "coin_collected");
        assert_eq!(events[0]["item_id"], "dungeon-coin-00");
        assert_eq!(events[1]["event"], "room_transition");
        assert_eq!(
            events[1]["destination_room_id"],
            DemoDungeonRoom::MossWalk.id()
        );
    }

    #[test]
    fn dungeon_transition_persists_the_completed_human_attempt_before_loading_next_room() {
        let save_path = temporary_test_path("dungeon-attempt-transition", "save.json");
        let history_path = save_path.parent().unwrap().join("history.jsonl");
        let selection = ScenarioSelection {
            mode: RoomMode::Dungeon,
            seed: 0,
            tier: AbilityTier::Baseline,
        };
        let mut client = ClientState::new_with_persistence(
            selection,
            None,
            false,
            Some(&history_path),
            Some((&save_path, false)),
        )
        .unwrap();
        // The tutorial exit needs a hop, so hold right and jump periodically.
        for tick in 0..600 {
            client.step_human(Action {
                move_x: 1,
                jump: tick % 30 < 8,
                ..Action::default()
            });
            if client.dungeon_run.unwrap().room == DemoDungeonRoom::MossWalk {
                break;
            }
        }
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::MossWalk);
        drop(client);

        let records = fs::read_to_string(&history_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let attempt = records
            .iter()
            .find(|record| record["schema"] == HUMAN_HISTORY_SCHEMA)
            .expect("room-local human attempt was persisted before transition");
        assert_eq!(
            attempt["dungeon_definition_id"],
            demo_dungeon_definition().id
        );
        assert_eq!(
            attempt["palette_generation_version"],
            DUNGEON_PALETTE_GENERATION_VERSION
        );
        assert_eq!(attempt["room_id"], DemoDungeonRoom::HollowLanding.id());
        assert_eq!(attempt["outcome"]["kind"], "success");
        assert_eq!(attempt["outcome"]["detail"], "east");
        assert!(records.iter().any(|record| {
            record["schema"] == "downwards-dungeon-progress-v2"
                && record["event"] == "room_transition"
        }));
    }

    #[test]
    fn dungeon_floor_lab_attempts_keep_provenance_without_progress_events() {
        let history_path = temporary_test_path("dungeon-floor-attempt", "history.jsonl");
        let room = DemoDungeonRoom::DashChasm;
        let selection = ScenarioSelection {
            mode: RoomMode::Challenge(ChallengeKind::DungeonFloor(room)),
            seed: 0,
            tier: AbilityTier::Baseline,
        };
        let mut client =
            ClientState::new_with_persistence(selection, None, false, Some(&history_path), None)
                .unwrap();
        client.step_human(Action {
            restart: true,
            ..Action::default()
        });
        drop(client);

        let records = fs::read_to_string(&history_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 1);
        let attempt = &records[0];
        assert_eq!(attempt["schema"], HUMAN_HISTORY_SCHEMA);
        assert_eq!(
            attempt["dungeon_definition_id"],
            demo_dungeon_definition().id
        );
        assert_eq!(
            attempt["palette_generation_version"],
            DUNGEON_PALETTE_GENERATION_VERSION
        );
        assert_eq!(
            attempt["level_id"],
            format!("dungeon-playtest:{}", room.id())
        );
        assert_eq!(attempt["room_id"], room.id());
        assert!(
            records
                .iter()
                .all(|record| { record["schema"] != "downwards-dungeon-progress-v2" })
        );
    }

    #[test]
    fn gallery_launch_is_dedicated_content_locked_and_conflict_checked() {
        let options = parse_launch_options(["--gallery"]).unwrap();
        assert_eq!(options.selection.mode, RoomMode::Gallery);
        assert_eq!(options.selection.seed, 0);
        assert_eq!(options.corpus_manifest, None);
        assert!(!options.allow_provisional_corpus);

        assert!(parse_launch_options(["--gallery", "--gallery"]).is_err());
        assert!(parse_launch_options(["--gallery", "--challenge", "hard"]).is_err());
        assert!(parse_launch_options(["--challenge", "tutorial", "--gallery"]).is_err());
        assert!(parse_launch_options(["--gallery", "--tier", "wall-jump"]).is_err());
        assert!(parse_launch_options(["--gallery", "--seed", "1"]).is_err());
        assert!(parse_launch_options(["--development", "--gallery"]).is_err());
        assert!(parse_launch_options(["--gallery", "--corpus=x"]).is_err());
    }

    #[test]
    fn calibrated_launch_is_seeded_locked_and_conflict_checked() {
        let default = parse_launch_options(["--calibrated"]).unwrap();
        assert_eq!(default.selection.mode, RoomMode::CalibratedGenerated);
        assert_eq!(default.selection.seed, 0);
        assert_eq!(default.selection.tier, AbilityTier::WallJump);

        let wrapped = parse_launch_options(["--calibrated=13"]).unwrap();
        assert_eq!(wrapped.selection.mode, RoomMode::CalibratedGenerated);
        assert_eq!(wrapped.selection.seed, 1);
        assert_eq!(wrapped.selection.tier, AbilityTier::WallJump);

        assert!(parse_launch_options(["--calibrated="]).is_err());
        assert!(parse_launch_options(["--calibrated", "--calibrated=1"]).is_err());
        assert!(parse_launch_options(["--calibrated", "--tier", "wall-jump"]).is_err());
        assert!(parse_launch_options(["--calibrated", "--seed", "1"]).is_err());
        assert!(parse_launch_options(["--calibrated", "--gallery"]).is_err());
        assert!(parse_launch_options(["--calibrated", "--challenge"]).is_err());
        assert!(parse_launch_options(["--calibrated", "--corpus=x"]).is_err());
    }

    #[test]
    fn curated_level_identifiers_are_three_words_and_unique_across_profiles() {
        let catalogue = load_playable_catalogue(None, false).unwrap();
        let mut identifiers = HashSet::new();
        let mut count = 0;
        for tier in [
            AbilityTier::Baseline,
            AbilityTier::WallJump,
            AbilityTier::Dash,
            AbilityTier::WallJumpAndDash,
        ] {
            for entry in catalogue.entries(tier) {
                let identifier = catalogue.name(entry);
                assert_eq!(identifier.split_whitespace().count(), 3);
                assert!(identifiers.insert(identifier.to_owned()));
                count += 1;
            }
        }
        assert_eq!(identifiers.len(), count);
    }

    #[test]
    fn level_menu_focuses_the_current_curated_entry() {
        let selection = ScenarioSelection {
            mode: RoomMode::Generated,
            seed: 10,
            tier: AbilityTier::WallJump,
        };
        let mut menu = LevelMenuState::focused_on(selection, 12);
        assert_eq!(menu.selected_index, 10);
        assert_eq!(menu.first_visible_index, 2);
        assert_eq!(menu.selected_scenario(), selection);

        menu.move_up();
        assert_eq!(menu.selected_scenario().seed, 9);
        menu.select_tier(AbilityTier::Dash, 6);
        assert_eq!(menu.selected_scenario().tier, AbilityTier::Dash);
        assert_eq!(menu.selected_scenario().seed, 5);

        menu.select_tier(AbilityTier::WallJump, 0);
        assert_eq!(menu.selected_scenario().tier, AbilityTier::WallJump);
        assert_eq!(menu.selected_scenario().mode, RoomMode::Development);
        assert_eq!(menu.selected_scenario().seed, 0);
    }

    #[test]
    fn level_menu_scrolls_continuously_and_clamps_at_catalogue_ends() {
        let mut menu = LevelMenuState::focused_on(
            ScenarioSelection {
                mode: RoomMode::Generated,
                seed: 11,
                tier: AbilityTier::Baseline,
            },
            12,
        );
        assert_eq!(menu.selected_index, 11);
        assert_eq!(menu.first_visible_index, 3);
        assert_eq!(menu.visible_indices().count(), LEVEL_MENU_VISIBLE_ROWS);

        menu.move_down();
        menu.page_down();
        assert_eq!(menu.selected_scenario().mode, RoomMode::Development);
        menu.page_up();
        assert_eq!(menu.selected_scenario().seed, 3);
        menu.move_home();
        assert_eq!(menu.selected_scenario(), ScenarioSelection::default());
        menu.page_down();
        assert_eq!(menu.selected_scenario().seed, 9);
        menu.move_end();
        assert_eq!(menu.selected_scenario().mode, RoomMode::Development);
    }

    #[test]
    fn dungeon_mode_menu_browses_dungeon_floors_and_launches_a_floor_lab() {
        let selection = ScenarioSelection {
            mode: RoomMode::Dungeon,
            seed: 0,
            tier: AbilityTier::Baseline,
        };
        let mut client = ClientState::new(selection, None, false).unwrap();

        client.open_level_menu();
        assert_eq!(client.browser_mode, BrowserMode::DungeonFloors);
        assert!(client.gallery_menu_visible());
        assert_eq!(client.gallery_menu.item_count, DemoDungeonRoom::ALL.len());
        assert_eq!(client.gallery_menu.selected_index, 0);

        let tempo_index = DemoDungeonRoom::ALL
            .iter()
            .position(|room| *room == DemoDungeonRoom::TempoHall)
            .unwrap();
        client.gallery_menu.selected_index = tempo_index;
        client.gallery_menu.ensure_selected_visible();
        assert!(client.refresh_gallery_menu_preview());
        assert!(client.level_menu_preview.simulation.is_some());
        client.play_gallery_selection().unwrap();
        assert_eq!(
            client.selection.mode,
            RoomMode::Challenge(ChallengeKind::DungeonFloor(DemoDungeonRoom::TempoHall))
        );
        assert!(client.dungeon_run.is_none());

        // Reopening the browser from the floor lab focuses the current floor.
        client.open_level_menu();
        assert_eq!(client.browser_mode, BrowserMode::DungeonFloors);
        assert_eq!(client.gallery_menu.selected_index, tempo_index);
    }

    #[test]
    fn floor_lab_door_continues_into_the_connected_room() {
        let selection = ScenarioSelection {
            mode: RoomMode::Challenge(ChallengeKind::DungeonFloor(DemoDungeonRoom::HollowLanding)),
            seed: 0,
            tier: AbilityTier::Baseline,
        };
        let mut client = ClientState::new(selection, None, false).unwrap();
        let report = StepReport {
            tick: client.simulation.tick(),
            events: vec![SimulationEvent::ExitReached {
                id: "east".to_owned(),
            }],
            digest: client.simulation.digest(),
        };
        assert!(client.observe_floor_lab_progress(&report));
        assert_eq!(
            client.selection.mode,
            RoomMode::Challenge(ChallengeKind::DungeonFloor(DemoDungeonRoom::MossWalk))
        );
        assert_eq!(client.simulation.entry_door(), Some("west"));
        // Completion never falls through to the generated catalogue.
        assert_eq!(client.next_level_selection(), None);
    }

    #[test]
    fn dungeon_v2_map_layout_places_every_room_distinctly() {
        let layout = dungeon_v2_map_layout();
        let dungeon = dungeon_v2_definition();
        assert_eq!(layout.len(), dungeon.instances.len());
        let mut seen = HashSet::new();
        for cell in layout.values() {
            assert!(seen.insert(*cell), "map layout reuses cell {cell:?}");
        }
    }

    #[test]
    fn dungeon_map_layout_places_every_room_distinctly() {
        let layout = dungeon_map_layout();
        assert_eq!(layout.len(), DemoDungeonRoom::ALL.len());
        let cells: HashSet<(i32, i32)> = layout.values().copied().collect();
        assert_eq!(cells.len(), layout.len(), "map cells must be distinct");
        assert_eq!(layout[&DemoDungeonRoom::HollowLanding], (0, 0));
    }

    #[test]
    fn dungeon_save_roundtrips_explored_rooms() {
        let mut explored = BTreeSet::from([DemoDungeonRoom::HollowLanding]);
        explored.insert(DemoDungeonRoom::MossWalk);
        let run = DungeonRunState::default();
        let save = DungeonSaveV1::from_run(run, None, &explored);
        let (restored_run, entry, restored_explored) = save.validate().unwrap();
        assert_eq!(restored_run, run);
        assert_eq!(entry, None);
        assert_eq!(restored_explored, explored);

        // Old saves without the field restore with the current room explored.
        let mut value = serde_json::to_value(&save).unwrap();
        value.as_object_mut().unwrap().remove("explored_rooms");
        let legacy: DungeonSaveV1 = serde_json::from_value(value).unwrap();
        let (_, _, legacy_explored) = legacy.validate().unwrap();
        assert_eq!(
            legacy_explored,
            BTreeSet::from([DemoDungeonRoom::HollowLanding])
        );
    }

    #[test]
    fn development_entry_is_an_ordinary_scrollable_row_after_curated_groups() {
        let mut menu = LevelMenuState::focused_on(ScenarioSelection::default(), 12);
        assert_eq!(menu.first_visible_index, 0);
        for _ in 0..9 {
            menu.move_down();
        }
        assert_eq!(menu.selected_scenario().seed, 9);
        assert_eq!(menu.first_visible_index, 1);
        assert!(!menu.visible_indices().contains(&12));
        menu.move_end();
        assert_eq!(menu.selected_scenario().mode, RoomMode::Development);
        assert_eq!(menu.first_visible_index, 4);
        assert!(menu.visible_indices().contains(&12));
    }

    #[test]
    fn next_level_wraps_within_the_curated_tier() {
        let selection = |mode, seed| ScenarioSelection {
            mode,
            seed,
            tier: AbilityTier::WallJump,
        };
        assert_eq!(
            selection(RoomMode::Development, 91).next_catalogue_level(6),
            selection(RoomMode::Generated, 0)
        );
        assert_eq!(
            selection(RoomMode::Generated, 0).next_catalogue_level(6),
            selection(RoomMode::Generated, 1)
        );
        assert_eq!(
            selection(RoomMode::Generated, 5).next_catalogue_level(6),
            selection(RoomMode::Generated, 0)
        );
        assert_eq!(
            selection(RoomMode::DeveloperGenerated, 9).next_catalogue_level(6),
            selection(RoomMode::Generated, 0)
        );
    }

    #[test]
    fn menu_preview_refreshes_only_when_the_exact_selection_changes() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        assert_eq!(
            client.level_menu_preview.selection,
            ScenarioSelection::default()
        );
        let original_preview = client
            .level_menu_preview
            .simulation
            .as_ref()
            .expect("default preview should load")
            as *const Simulation;
        assert!(!client.refresh_level_menu_preview());
        assert_eq!(
            client.level_menu_preview.simulation.as_ref().unwrap() as *const Simulation,
            original_preview,
            "an unchanged menu frame must retain the cached preview"
        );

        client.level_menu.move_down();
        assert!(client.refresh_level_menu_preview());
        assert_eq!(client.level_menu_preview.selection.seed, 1);
        let expected = load_scenario(client.level_menu.selected_scenario(), &client.catalogue)
            .unwrap()
            .0
            .digest();
        assert_eq!(
            client
                .level_menu_preview
                .simulation
                .as_ref()
                .unwrap()
                .digest(),
            expected
        );

        client.select_menu_tier(AbilityTier::Dash);
        assert!(client.refresh_level_menu_preview());
        assert_eq!(client.level_menu_preview.selection.tier, AbilityTier::Dash);
        assert_eq!(
            client
                .level_menu_preview
                .simulation
                .as_ref()
                .unwrap()
                .abilities(),
            AbilityTier::Dash.abilities()
        );
        assert!(!client.refresh_level_menu_preview());
    }

    #[test]
    fn gallery_menu_navigation_is_bounded_and_empty_safe() {
        let mut empty = GalleryMenuState::focused_on(99, 0);
        empty.move_up();
        empty.move_down();
        empty.page_up();
        empty.page_down();
        empty.move_home();
        empty.move_end();
        assert_eq!(empty.visible_indices(), 0..0);
        assert_eq!(empty.selected_scenario(), None);
        assert_eq!(gallery_adjacent_index(0, 0, true), None);

        let mut menu = GalleryMenuState::focused_on(99, 12);
        assert_eq!(menu.selected_index, 11);
        menu.move_home();
        assert_eq!(menu.selected_index, 0);
        menu.move_up();
        assert_eq!(menu.selected_index, 0);
        menu.move_end();
        assert_eq!(menu.selected_index, 11);
        menu.move_down();
        assert_eq!(menu.selected_index, 11);
        assert_eq!(gallery_adjacent_index(0, 12, false), Some(11));
        assert_eq!(gallery_adjacent_index(11, 12, true), Some(0));
        assert_eq!(gallery_adjacent_index(u64::MAX, 12, true), Some(1));
    }

    #[test]
    fn movement_tuning_is_game_wide_and_applies_at_an_attempt_boundary() {
        let course = ScenarioSelection {
            mode: RoomMode::Gallery,
            seed: 12,
            tier: AbilityTier::Baseline,
        };
        let mut client = ClientState::new(course, None, false).unwrap();
        assert!(client.is_movement_course());
        assert_eq!(client.movement_tuning, MovementTuning::GAMEPLAY_DEFAULT);
        assert_eq!(
            client.simulation.movement_tuning(),
            MovementTuning::GAMEPLAY_DEFAULT
        );

        assert!(client.open_movement_tuning_menu());
        client.adjust_movement_tuning(1);
        client.cancel_movement_tuning_menu();
        assert_eq!(client.movement_tuning, MovementTuning::GAMEPLAY_DEFAULT);
        assert_eq!(
            client.simulation.movement_tuning(),
            MovementTuning::GAMEPLAY_DEFAULT
        );

        client.step_human(Action {
            move_x: 1,
            ..Action::default()
        });
        assert!(client.open_movement_tuning_menu());
        client.adjust_movement_tuning(1);
        assert_eq!(client.movement_tuning.top_speed_pixels_per_second, 115);
        assert_eq!(
            client.simulation.movement_tuning(),
            MovementTuning::GAMEPLAY_DEFAULT,
            "the paused menu edits a draft until it closes"
        );
        client.move_movement_tuning_selection(1);
        client.adjust_movement_tuning(-1);
        assert_eq!(client.movement_tuning.acceleration_milliseconds, 67);
        client.close_movement_tuning_menu();
        assert_eq!(client.simulation.movement_tuning(), client.movement_tuning);
        assert_eq!(client.simulation.room_tick(), 0);
        assert!(matches!(
            client
                .human_recorder
                .last_completed
                .as_ref()
                .map(|attempt| &attempt.outcome),
            Some(AttemptOutcome::Reset)
        ));
        assert_eq!(
            client
                .human_recorder
                .last_completed
                .as_ref()
                .unwrap()
                .initial
                .movement_tuning(),
            MovementTuning::GAMEPLAY_DEFAULT
        );

        assert!(client.open_movement_tuning_menu());
        client.reset_movement_tuning_draft();
        assert_eq!(client.movement_tuning, MovementTuning::GAMEPLAY_DEFAULT);
        client.close_movement_tuning_menu();

        client.switch_to(ScenarioSelection::default()).unwrap();
        assert!(!client.is_movement_course());
        assert_eq!(
            client.simulation.movement_tuning(),
            MovementTuning::GAMEPLAY_DEFAULT
        );
        assert!(client.open_movement_tuning_menu());
        assert_eq!(client.movement_tuning, MovementTuning::GAMEPLAY_DEFAULT);
    }

    #[test]
    fn tier_and_room_mode_transitions_preserve_other_selection_fields() {
        let mut selection = ScenarioSelection::default();
        selection.select_tier(AbilityTier::WallJumpAndDash);
        selection.toggle_mode();
        assert_eq!(selection.mode, RoomMode::Development);
        assert_eq!(selection.seed, DEFAULT_SEED);
        assert_eq!(selection.tier, AbilityTier::WallJumpAndDash);
        assert!(selection.tier.abilities().wall_jump);
        assert!(selection.tier.abilities().dash);
    }

    #[test]
    fn challenge_selection_ignores_mode_and_tier_mutators() {
        let catalogue = load_playable_catalogue(None, false).unwrap();
        let mut selection = ScenarioSelection {
            mode: RoomMode::Challenge(ChallengeKind::Medium),
            seed: 0,
            tier: AbilityTier::WallJump,
        };

        selection.select_tier(AbilityTier::WallJumpAndDash);
        selection.select_available_tier(AbilityTier::Dash, &catalogue);
        selection.toggle_mode();
        selection = selection_from_hotkeys(selection, &catalogue);

        assert_eq!(selection.mode, RoomMode::Challenge(ChallengeKind::Medium));
        assert_eq!(selection.tier, AbilityTier::WallJump);
        assert!(!selection.tier.abilities().dash);
    }

    #[test]
    fn kit_tabs_are_explicit_and_left_right_cycle_all_loadouts() {
        assert_eq!(kit_tab_label(AbilityTier::Baseline), "1 BASIC");
        assert_eq!(kit_tab_label(AbilityTier::WallJump), "2 WALL JUMP");
        assert_eq!(kit_tab_label(AbilityTier::Dash), "3 DASH");
        assert_eq!(kit_tab_label(AbilityTier::WallJumpAndDash), "4 BOTH");

        let mut tier = AbilityTier::Baseline;
        for expected in [
            AbilityTier::WallJump,
            AbilityTier::Dash,
            AbilityTier::WallJumpAndDash,
            AbilityTier::Baseline,
        ] {
            tier = next_ability_tier(tier);
            assert_eq!(tier, expected);
        }
        assert_eq!(
            previous_ability_tier(AbilityTier::Baseline),
            AbilityTier::WallJumpAndDash
        );
        assert_eq!(
            previous_ability_tier(AbilityTier::Dash),
            AbilityTier::WallJump
        );
    }

    #[test]
    fn menu_left_right_kit_selection_preserves_the_selected_level_row() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        client.level_menu.move_down();
        client.level_menu.move_down();
        assert_eq!(client.level_menu.selected_index, 2);

        client.select_next_menu_tier();
        assert_eq!(client.level_menu.tier, AbilityTier::WallJump);
        assert_eq!(client.level_menu.selected_index, 2);
        assert!(client.refresh_level_menu_preview());
        assert_eq!(
            client.level_menu_preview.selection.tier,
            AbilityTier::WallJump
        );

        client.select_previous_menu_tier();
        assert_eq!(client.level_menu.tier, AbilityTier::Baseline);
        assert_eq!(client.level_menu.selected_index, 2);
    }

    #[test]
    fn preview_mechanics_distinguish_enabled_kit_from_verified_route_usage() {
        assert_eq!(
            verified_route_mechanics(AbilityTier::Baseline, Some((0, 0))),
            "KIT WALL:LOCKED DASH:LOCKED | ROUTE WJ 0 DASH 0"
        );
        assert_eq!(
            verified_route_mechanics(AbilityTier::WallJump, Some((3, 0))),
            "KIT WALL:ON DASH:LOCKED | ROUTE WJ 3 DASH 0"
        );
        assert_eq!(
            verified_route_mechanics(AbilityTier::WallJumpAndDash, Some((2, 1))),
            "KIT WALL:ON DASH:ON | ROUTE WJ 2 DASH 1"
        );
        assert!(verified_route_mechanics(AbilityTier::Dash, None).contains("NO VERIFIED ROUTE"));
    }

    #[test]
    fn wall_slide_cue_requires_the_ability_active_slide_and_wall_contact() {
        assert_eq!(
            wall_slide_cue(true, true, Some(WallSide::Left)),
            Some(WallSide::Left)
        );
        assert_eq!(wall_slide_cue(false, true, Some(WallSide::Left)), None);
        assert_eq!(wall_slide_cue(true, false, Some(WallSide::Right)), None);
        assert_eq!(wall_slide_cue(true, true, None), None);
    }

    #[test]
    fn all_room_modes_construct_with_the_explicit_selected_loadout() {
        let catalogue = load_playable_catalogue(None, false).unwrap();
        for mode in [
            RoomMode::Generated,
            RoomMode::DeveloperGenerated,
            RoomMode::Development,
        ] {
            let selection = ScenarioSelection {
                mode,
                seed: if mode == RoomMode::Generated {
                    0
                } else {
                    0x5eed
                },
                tier: AbilityTier::Dash,
            };
            let (simulation, provenance) = load_scenario(selection, &catalogue).unwrap();
            assert_eq!(simulation.abilities(), AbilityTier::Dash.abilities());
            if mode != RoomMode::Development {
                let provenance = provenance.expect("generated room should retain provenance");
                assert_eq!(
                    provenance.generation_version,
                    COMPOSITIONAL_GENERATION_VERSION
                );
            } else {
                assert_eq!(provenance, None);
            }
        }
    }

    #[test]
    fn challenge_starts_in_gameplay_with_the_content_locked_no_dash_loadout() {
        let selection = ScenarioSelection {
            mode: RoomMode::Challenge(ChallengeKind::Hard),
            seed: 99,
            tier: AbilityTier::WallJumpAndDash,
        };
        let mut client = ClientState::new(selection, None, false).unwrap();

        assert!(!client.level_menu_visible());
        assert_eq!(client.selection.seed, 0);
        assert_eq!(client.selection.tier, AbilityTier::WallJump);
        assert_eq!(client.simulation.abilities(), HARD_NO_DASH_ABILITIES);
        assert!(client.simulation.abilities().wall_jump);
        assert!(!client.simulation.abilities().dash);
        assert_eq!(client.selected_target_id(), Some(HARD_NO_DASH_TARGET));
        assert_eq!(
            client.stats_key(client.selection),
            "challenge:hard-no-dash:v1"
        );

        client.open_level_menu();
        assert!(client.level_menu_visible());
        assert!(!client.level_menu.selected_scenario().mode.is_challenge());
    }

    #[test]
    fn medium_challenge_has_distinct_content_target_label_and_stats() {
        let selection = ScenarioSelection {
            mode: RoomMode::Challenge(ChallengeKind::Medium),
            seed: 99,
            tier: AbilityTier::WallJumpAndDash,
        };
        let client = ClientState::new(selection, None, false).unwrap();

        assert!(!client.level_menu_visible());
        assert_eq!(client.selection.seed, 0);
        assert_eq!(client.selection.tier, AbilityTier::WallJump);
        assert_eq!(client.simulation.abilities(), MEDIUM_NO_DASH_ABILITIES);
        assert!(client.simulation.abilities().wall_jump);
        assert!(!client.simulation.abilities().dash);
        assert_eq!(client.selected_target_id(), Some(MEDIUM_NO_DASH_TARGET));
        assert_eq!(
            client.level_name(client.selection),
            MEDIUM_NO_DASH_LEVEL_IDENTIFIER
        );
        assert_eq!(
            client.stats_key(client.selection),
            "challenge:medium-no-dash:v1"
        );
    }

    #[test]
    fn gallery_starts_in_its_dedicated_browser_and_uses_content_metadata() {
        let selection = ScenarioSelection {
            mode: RoomMode::Gallery,
            seed: 0,
            tier: AbilityTier::WallJumpAndDash,
        };
        let mut client = ClientState::new(selection, None, false).unwrap();
        let first = calibration_gallery()[0];

        assert!(client.level_menu_visible());
        assert!(client.gallery_menu_visible());
        assert_eq!(client.selection.mode, RoomMode::Gallery);
        assert_eq!(client.selection.seed, 0);
        assert_eq!(client.selection.tier, tier_for_abilities(first.abilities()));
        assert_eq!(client.simulation.abilities(), first.abilities());
        assert!(!client.simulation.abilities().dash);
        assert_eq!(client.selected_target_id(), Some(first.target()));
        assert_eq!(client.generated_provenance, None);
        assert_eq!(client.stats_key(client.selection), "gallery:cal-01");

        client.close_level_menu();
        assert!(!client.level_menu_visible());
        client.open_level_menu();
        assert!(client.gallery_menu_visible());

        client.gallery_menu.move_end();
        assert!(client.refresh_gallery_menu_preview());
        client.play_gallery_selection().unwrap();
        assert!(!client.level_menu_visible());
        assert_eq!(
            client.current_gallery_entry().map(CalibrationLevel::id),
            calibration_gallery()
                .last()
                .copied()
                .map(CalibrationLevel::id)
        );
    }

    #[test]
    fn gallery_entries_have_distinct_stats_and_reject_invalid_indices() {
        let catalogue = load_playable_catalogue(None, false).unwrap();
        let mut stats_keys = HashSet::new();
        for (index, level) in calibration_gallery().iter().copied().enumerate() {
            let selection = ScenarioSelection {
                mode: RoomMode::Gallery,
                seed: index as u64,
                tier: AbilityTier::WallJumpAndDash,
            }
            .canonicalized();
            let (scenario, provenance) = load_scenario(selection, &catalogue).unwrap();
            assert_eq!(scenario.abilities(), level.abilities());
            assert!(!scenario.abilities().dash);
            assert_eq!(provenance, None);

            let client = ClientState::new(selection, None, false).unwrap();
            assert!(stats_keys.insert(client.stats_key(selection)));
        }

        let missing = ScenarioSelection {
            mode: RoomMode::Gallery,
            seed: calibration_gallery().len() as u64,
            tier: AbilityTier::Baseline,
        };
        assert!(load_scenario(missing, &catalogue).is_err());
    }

    #[test]
    fn playback_transport_supports_pause_frame_step_completion_and_restart() {
        let mut transport = PlaybackTransport::new(2);
        assert_eq!(transport.phase, PlaybackPhase::Playing);
        assert!(transport.should_advance(false));

        assert!(transport.toggle_play_pause());
        assert_eq!(transport.phase, PlaybackPhase::Paused);
        assert!(!transport.should_advance(false));
        assert!(transport.should_advance(true));
        transport.mark_advanced();
        assert_eq!(transport.next_frame, 1);
        assert_eq!(transport.phase, PlaybackPhase::Paused);

        assert!(transport.toggle_play_pause());
        transport.mark_advanced();
        assert_eq!(transport.phase, PlaybackPhase::Complete);
        assert!(!transport.should_advance(true));

        assert!(transport.toggle_play_pause());
        assert_eq!(transport.phase, PlaybackPhase::Playing);
        assert_eq!(transport.next_frame, 0);
    }

    #[test]
    fn client_starts_human_controlled_and_solve_request_can_be_cancelled() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        assert!(client.human_controlled());
        assert!(client.level_menu_visible());
        assert!(!client.solve_requested());

        client.request_solve();
        assert!(client.solve_requested());
        assert!(!client.human_controlled());
        assert!(client.cancel_replay());
        assert!(client.human_controlled());
        assert!(client.replay_notice.is_some());
    }

    #[test]
    fn menu_activation_loads_the_named_selection_and_waits_for_navigation_release() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        client.level_menu.move_down();
        client.select_menu_tier(AbilityTier::Dash);
        let expected = ScenarioSelection {
            mode: RoomMode::Generated,
            seed: 1,
            tier: AbilityTier::Dash,
        };

        client.play_menu_selection().unwrap();
        assert!(!client.level_menu_visible());
        assert_eq!(client.selection, expected);
        assert_eq!(client.simulation.abilities(), AbilityTier::Dash.abilities());
        assert!(!client.acknowledge_menu_input_release(true));
        assert!(client.acknowledge_menu_input_release(false));
        assert!(client.acknowledge_menu_input_release(true));
    }

    #[test]
    fn replay_frames_use_the_client_simulation_step_path() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        let initial = client.simulation.clone();
        let replay = Replay::record(&initial, [Action::default(); 2]);
        let first_digest = replay.frames[0].expected_digest;
        let first_event_digest = replay.frames[0].expected_event_digest;
        let mut independently_stepped = initial.clone();
        let first_report = independently_stepped.step(Action::default());
        client.replay_mode = ReplayMode::Playback(Box::new(ReplayPlayback {
            initial,
            transport: PlaybackTransport::new(replay.frames.len()),
            replay,
            expected_objective: None,
            origin: ReplayOrigin::Solver(SolverDiagnostics {
                search_stats: SearchStats::default(),
                provisional_band: ComplexityBand::Gentle,
                temporal_robustness: None,
                accepted_wall_jumps: 0,
                accepted_dashes: 0,
            }),
        }));

        client.advance_replay(false);
        assert_eq!(client.simulation.digest(), first_digest);
        assert_eq!(first_event_digest, digest_events(&first_report.events));
        let ReplayMode::Playback(playback) = &client.replay_mode else {
            panic!("a matching replay frame should remain active");
        };
        assert_eq!(playback.transport.next_frame, 1);
        assert_eq!(playback.transport.phase, PlaybackPhase::Playing);
        assert_eq!(
            client.stats_for(client.selection),
            LevelStats::default(),
            "solver playback must not count as a human run"
        );
    }

    #[test]
    fn human_recording_captures_actions_and_closes_the_attempt_at_reset() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        let initial = client.simulation.clone();
        let actions = [
            Action::default(),
            Action {
                move_x: 1,
                ..Action::default()
            },
            Action {
                restart: true,
                ..Action::default()
            },
        ];
        let expected_replay = Replay::record(&initial, actions);
        for action in actions {
            client.step_human(action);
        }

        let completed = client
            .human_recorder
            .last_completed
            .as_ref()
            .expect("manual reset should complete an attempt");
        assert_eq!(completed.replay.actions().collect::<Vec<_>>(), actions);
        assert_eq!(completed.replay, expected_replay);
        assert_eq!(completed.outcome, AttemptOutcome::Reset);
        assert_eq!(
            completed
                .replay
                .verify(&completed.initial)
                .unwrap()
                .final_digest,
            client.simulation.digest()
        );
        let current = client
            .human_recorder
            .current
            .as_ref()
            .expect("reset should begin a fresh attempt");
        assert!(current.frames.is_empty());
        assert_eq!(current.initial.digest(), client.simulation.digest());

        client.start_last_human_replay();
        let ReplayMode::Playback(playback) = &client.replay_mode else {
            panic!("completed human attempt should enter shared playback");
        };
        assert!(matches!(
            playback.origin,
            ReplayOrigin::Human(AttemptOutcome::Reset)
        ));
        assert_eq!(playback.transport.total_frames, actions.len());
        let stats_before_replay = client.stats_for(client.selection);
        assert_eq!(
            stats_before_replay,
            LevelStats {
                attempts: 1,
                ..LevelStats::default()
            }
        );
        for _ in 0..actions.len() {
            client.advance_replay(false);
        }
        assert_eq!(
            client.stats_for(client.selection),
            stats_before_replay,
            "replaying a human run must not count it twice"
        );
    }

    #[test]
    fn persistent_history_records_raw_space_duration_and_delivered_jump_ticks() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "downwards-human-history-test-{}-{unique}",
            std::process::id()
        ));
        let path = root.join("attempts.jsonl");
        let mut client =
            ClientState::new_with_history(ScenarioSelection::default(), None, false, Some(&path))
                .unwrap();
        let sample =
            |render_frame, sampled_at_session_us, pressed, released, held| RawJumpInputSample {
                render_frame,
                sampled_at_session_us,
                keys: [
                    RawJumpKeySample {
                        key: HumanJumpKey::Space,
                        pressed,
                        released,
                        held,
                    },
                    RawJumpKeySample {
                        key: HumanJumpKey::Z,
                        pressed: false,
                        released: false,
                        held: false,
                    },
                    RawJumpKeySample {
                        key: HumanJumpKey::Up,
                        pressed: false,
                        released: false,
                        held: false,
                    },
                ],
            };

        client.observe_human_jump_frame(sample(10, 100, true, false, true));
        client.step_human(Action {
            jump: true,
            ..Action::default()
        });
        client.observe_human_jump_frame(sample(11, 116, false, false, true));
        client.step_human(Action {
            jump: true,
            ..Action::default()
        });
        client.observe_human_jump_frame(sample(12, 133, false, true, false));
        client.step_human(Action::default());
        client.step_human(Action {
            restart: true,
            ..Action::default()
        });

        let attempt = client.human_recorder.last_completed.as_ref().unwrap();
        assert_eq!(attempt.raw_jump_presses.len(), 1);
        assert_eq!(attempt.raw_jump_presses[0].key, HumanJumpKey::Space);
        assert_eq!(attempt.raw_jump_presses[0].sampled_duration_us, Some(33));
        assert_eq!(attempt.raw_jump_presses[0].sampled_frames_held, 2);
        assert_eq!(
            delivered_jump_spans(&attempt.replay.frames),
            vec![DeliveredJumpSpanV1 {
                start_tick: 1,
                ticks: 2,
            }]
        );

        let persisted = fs::read_to_string(&path).unwrap();
        let lines = persisted.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1);
        let record: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(record["schema"], HUMAN_HISTORY_SCHEMA);
        assert_eq!(record["human_jump_input_policy_version"], 4);
        assert_eq!(
            record["player_movement_policy_version"],
            PLAYER_MOVEMENT_POLICY_VERSION
        );
        assert_eq!(record["movement_profile"], "gameplay-v2");
        assert_eq!(
            record["movement_tuning"]["top_speed_pixels_per_second"],
            110
        );
        assert_eq!(record["movement_tuning"]["braking_milliseconds"], 203);
        assert_eq!(record["raw_jump_presses"][0]["key"], "space");
        assert_eq!(record["raw_jump_presses"][0]["sampled_duration_us"], 33);
        assert_eq!(record["raw_jump_presses"][0]["sampled_frames_held"], 2);
        assert_eq!(record["delivered_jump_spans"][0]["start_tick"], 1);
        assert_eq!(record["delivered_jump_spans"][0]["ticks"], 2);
        assert_eq!(record["frames"].as_array().unwrap().len(), 4);

        drop(client);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn movement_tuning_changes_are_persisted_even_without_a_later_attempt() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "downwards-movement-tuning-history-test-{}-{unique}",
            std::process::id()
        ));
        let path = root.join("attempts.jsonl");
        let course = ScenarioSelection {
            mode: RoomMode::Gallery,
            seed: 12,
            tier: AbilityTier::Baseline,
        };
        let mut client = ClientState::new_with_history(course, None, false, Some(&path)).unwrap();
        assert!(client.open_movement_tuning_menu());
        client.adjust_movement_tuning(1);
        client.close_movement_tuning_menu();
        drop(client);

        let persisted = fs::read_to_string(&path).unwrap();
        let records = persisted
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["schema"], HUMAN_HISTORY_SCHEMA);
        assert_eq!(records[0]["movement_profile"], "gameplay-v2");
        assert_eq!(
            records[0]["movement_tuning"]["top_speed_pixels_per_second"],
            110
        );
        assert_eq!(records[1]["schema"], "downwards-movement-tuning-v1");
        assert_eq!(
            records[1]["player_movement_policy_version"],
            PLAYER_MOVEMENT_POLICY_VERSION
        );
        assert_eq!(records[1]["tuning"]["top_speed_pixels_per_second"], 115);
        assert_eq!(records[1]["tuning"]["acceleration_milliseconds"], 72);
        assert_eq!(records[1]["tuning"]["braking_milliseconds"], 203);
        assert_eq!(records[1]["tuning"]["wall_ascent_carry_percent"], 50);
        assert_eq!(records[1]["tuning"]["wall_carry_percent"], 50);
        assert_eq!(records[1]["tuning"]["wall_momentum_milliseconds"], 250);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn history_recorder_ignores_a_release_after_an_attempt_boundary() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        client.observe_human_jump_frame(RawJumpInputSample {
            render_frame: 1,
            sampled_at_session_us: 10,
            keys: [
                RawJumpKeySample {
                    key: HumanJumpKey::Space,
                    pressed: false,
                    released: true,
                    held: false,
                },
                RawJumpKeySample {
                    key: HumanJumpKey::Z,
                    pressed: false,
                    released: false,
                    held: false,
                },
                RawJumpKeySample {
                    key: HumanJumpKey::Up,
                    pressed: false,
                    released: false,
                    held: false,
                },
            ],
        });

        assert!(
            client
                .human_recorder
                .current
                .as_ref()
                .unwrap()
                .raw_jump_presses
                .is_empty()
        );
    }

    #[test]
    fn session_stats_count_only_completed_human_runs_and_keep_the_fastest_clear() {
        let mut stats = LevelStats::default();
        stats.observe_human_step(
            &[SimulationEvent::PickupCollected {
                id: "coin".to_owned(),
            }],
            None,
            None,
        );
        assert_eq!(stats.coins_collected, 1);
        assert_eq!(stats.attempts, 0);

        stats.observe_human_step(
            &[],
            Some(&AttemptOutcome::Died(DeathReason::Hazard {
                tile_x: 2,
                tile_y: 16,
            })),
            Some(45),
        );
        stats.observe_human_step(
            &[],
            Some(&AttemptOutcome::Exit("down".to_owned())),
            Some(120),
        );
        stats.observe_human_step(
            &[],
            Some(&AttemptOutcome::Exit("down".to_owned())),
            Some(90),
        );
        stats.observe_human_step(&[], Some(&AttemptOutcome::Reset), Some(10));
        assert_eq!(
            stats,
            LevelStats {
                attempts: 4,
                deaths: 1,
                clears: 2,
                best_clear_ticks: Some(90),
                coins_collected: 1,
            }
        );
        assert_eq!(format_best_clear(stats.best_clear_ticks), "1.50s/90t");

        let development_a = ScenarioSelection {
            mode: RoomMode::Development,
            seed: 12,
            tier: AbilityTier::Dash,
        };
        let development_b = ScenarioSelection {
            seed: 800,
            ..development_a
        };
        let client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        assert_eq!(
            client.stats_key(development_a),
            client.stats_key(development_b)
        );
    }

    #[test]
    fn only_the_selected_target_door_counts_as_a_clear() {
        let reached = |id: &str| vec![SimulationEvent::ExitReached { id: id.to_owned() }];
        assert_eq!(
            attempt_outcome(&reached("port-east"), Some("port-floor")),
            Some(AttemptOutcome::WrongDoor("port-east".to_owned()))
        );
        assert_eq!(
            attempt_outcome(&reached("port-floor"), Some("port-floor")),
            Some(AttemptOutcome::Exit("port-floor".to_owned()))
        );

        let mut stats = LevelStats::default();
        stats.observe_human_step(
            &[],
            Some(&AttemptOutcome::WrongDoor("port-east".to_owned())),
            Some(30),
        );
        assert_eq!(stats.attempts, 1);
        assert_eq!(stats.clears, 0);
        assert_eq!(stats.best_clear_ticks, None);
    }

    #[test]
    fn playback_rejects_tampered_transient_events_with_matching_state() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        let initial = client.simulation.clone();
        let restart = Action {
            restart: true,
            ..Action::default()
        };
        let mut replay = Replay::record(&initial, [restart]);
        replay.frames[0].expected_event_digest = digest_events(&[]);
        client.replay_mode = ReplayMode::Playback(Box::new(ReplayPlayback {
            initial,
            transport: PlaybackTransport::new(replay.frames.len()),
            replay,
            expected_objective: None,
            origin: ReplayOrigin::Human(AttemptOutcome::Reset),
        }));

        client.advance_replay(false);

        assert!(matches!(client.replay_mode, ReplayMode::Human));
        let notice = client
            .replay_notice
            .as_ref()
            .expect("event divergence should be visible to the player");
        assert_eq!(notice.title, "PLAYBACK DIVERGED");
        assert!(notice.detail.contains("event digest"));
        assert!(notice.detail.contains("Reset"));
    }

    #[test]
    fn scenario_switch_clears_recording_boundaries_and_archives() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        client.step_human(Action {
            restart: true,
            ..Action::default()
        });
        assert!(client.human_recorder.last_completed.is_some());

        let next = client
            .selection
            .next_catalogue_level(client.catalogue.entries(client.selection.tier).len());
        client.switch_to(next).unwrap();
        assert!(client.human_recorder.last_completed.is_none());
        assert!(client.human_recorder.last_successful.is_none());
        let current = client.human_recorder.current.as_ref().unwrap();
        assert!(current.frames.is_empty());
        assert_eq!(current.initial.digest(), client.simulation.digest());
    }

    #[test]
    fn default_showcase_live_solves_the_target_door_under_current_movement() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        let source_door = client.current_entry().unwrap().source_door_id().to_owned();
        assert_eq!(client.simulation.entry_door(), Some(source_door.as_str()));
        let exact_initial_digest = client.simulation.digest();
        client.request_solve();
        client.perform_requested_solve();

        let ReplayMode::Playback(playback) = &client.replay_mode else {
            panic!("default showcase should produce a replay");
        };
        let witness_actions = playback.replay.actions().collect::<Vec<_>>();
        let frame_count = playback.transport.total_frames;
        let Some(ReplayObjective::Door(expected_exit)) = playback.expected_objective.clone() else {
            panic!("curated witness should target an exact door");
        };
        let ReplayOrigin::CatalogueRoute {
            band,
            stored_witness,
            search_stats,
            ..
        } = &playback.origin
        else {
            panic!("V should install curated route provenance");
        };
        assert!(band.is_none_or(|band| !catalogue_band_label(band).is_empty()));
        assert!(
            !*stored_witness,
            "historical route actions are policy-bound"
        );
        assert!(search_stats.is_some());
        assert!(frame_count > 0);
        assert_eq!(client.simulation.digest(), exact_initial_digest);
        assert_eq!(client.simulation.digest(), playback.replay.initial_digest);

        for _ in 0..frame_count {
            client.advance_replay(false);
        }
        assert_eq!(
            client.simulation.reached_exit(),
            Some(expected_exit.as_str())
        );
        assert!(client.replay_complete());

        client.switch_to(ScenarioSelection::default()).unwrap();
        for action in witness_actions {
            client.step_human(action);
        }
        let successful = client
            .human_recorder
            .last_successful
            .as_ref()
            .expect("a human-driven solver witness should be retained as a success");
        assert_eq!(
            successful.outcome,
            AttemptOutcome::Exit(expected_exit.clone())
        );

        // A later failed attempt must not displace the retained successful route.
        client.step_human(Action {
            restart: true,
            ..Action::default()
        });
        client.step_human(Action {
            restart: true,
            ..Action::default()
        });
        assert_eq!(
            client
                .human_recorder
                .last_completed
                .as_ref()
                .unwrap()
                .outcome,
            AttemptOutcome::Reset
        );
        assert!(matches!(
            client.human_recorder.preferred().unwrap().outcome,
            AttemptOutcome::Exit(_)
        ));
        client.start_last_human_replay();
        let ReplayMode::Playback(playback) = &client.replay_mode else {
            panic!("successful human attempt should use shared playback");
        };
        assert!(matches!(
            playback.origin,
            ReplayOrigin::Human(AttemptOutcome::Exit(_))
        ));
    }

    #[test]
    fn challenge_v_installs_and_plays_each_stored_exact_witness() {
        for kind in [ChallengeKind::Hard, ChallengeKind::Medium] {
            let selection = ScenarioSelection {
                mode: RoomMode::Challenge(kind),
                seed: 0,
                tier: AbilityTier::WallJump,
            };
            let mut client = ClientState::new(selection, None, false).unwrap();
            let initial_digest = client.simulation.digest();
            client.request_solve();
            client.perform_requested_solve();

            let ReplayMode::Playback(playback) = &client.replay_mode else {
                panic!(
                    "challenge should install its stored witness: {:?}",
                    client.replay_notice.as_ref().map(|notice| &notice.detail)
                );
            };
            assert!(matches!(playback.origin, ReplayOrigin::Challenge(actual) if actual == kind));
            assert_eq!(
                playback.expected_objective,
                Some(ReplayObjective::Exit(kind.target().to_owned()))
            );
            assert_eq!(client.simulation.digest(), initial_digest);
            assert_eq!(client.simulation.digest(), playback.replay.initial_digest);
            assert!(playback.replay.actions().all(|action| !action.dash));
            let frame_count = playback.transport.total_frames;
            assert!(frame_count > 0);

            for _ in 0..frame_count {
                client.advance_replay(false);
            }
            assert_eq!(client.simulation.reached_exit(), Some(kind.target()));
            assert!(client.replay_complete());
        }
    }

    #[test]
    fn dungeon_floor_lab_uses_exact_entry_inventory_objective_and_witness() {
        for room in [
            DemoDungeonRoom::HollowLanding,
            DemoDungeonRoom::ClimberVault,
            DemoDungeonRoom::DashChasm,
            DemoDungeonRoom::CrownSanctum,
        ] {
            let spec = dungeon_floor_route_spec(room);
            let kind = ChallengeKind::DungeonFloor(room);
            let selection = ScenarioSelection {
                mode: RoomMode::Challenge(kind),
                seed: 0,
                tier: AbilityTier::Baseline,
            };
            let mut client = ClientState::new(selection, None, false).unwrap();
            assert!(client.dungeon_run.is_none());
            assert!(client.dungeon_persistence.is_none());
            assert_eq!(client.simulation.room().id(), room.id());
            assert_eq!(client.simulation.entry_door(), spec.entry_door);
            assert_eq!(client.simulation.abilities(), spec.inventory.abilities());
            assert_eq!(
                client.selection.tier,
                tier_for_abilities(spec.inventory.abilities())
            );
            assert_eq!(
                client.stats_key(client.selection),
                format!("dungeon-playtest:{}", room.id())
            );

            client.request_solve();
            client.perform_requested_solve();
            let ReplayMode::Playback(playback) = &client.replay_mode else {
                panic!(
                    "{} should install its checked floor witness: {:?}",
                    room.id(),
                    client.replay_notice.as_ref().map(|notice| &notice.detail)
                );
            };
            assert_eq!(playback.expected_objective, Some(kind.objective()));
            assert!(matches!(playback.origin, ReplayOrigin::Challenge(actual) if actual == kind));
            let frame_count = playback.transport.total_frames;
            assert!(frame_count > 0);
            for _ in 0..frame_count {
                client.advance_replay(false);
            }
            // A finished floor-lab goal returns control to the human: either
            // the objective is satisfied in place (pickup goals) or the door
            // goal already carried the lab into the connected room.
            assert!(client.human_controlled());
            let transitioned = client.selection.mode
                != RoomMode::Challenge(ChallengeKind::DungeonFloor(room))
                || client.simulation.room().id() != room.id();
            assert!(
                kind.objective().is_satisfied_by(&client.simulation) || transitioned,
                "{} witness ended neither on its objective nor in a connected room",
                room.id()
            );
        }
    }

    #[test]
    fn gallery_v_installs_and_plays_every_registered_exact_witness() {
        for (index, level) in calibration_gallery().iter().copied().enumerate() {
            let selection = ScenarioSelection {
                mode: RoomMode::Gallery,
                seed: index as u64,
                tier: AbilityTier::Baseline,
            };
            let mut client = ClientState::new(selection, None, false).unwrap();
            client.request_solve();
            client.perform_requested_solve();

            let ReplayMode::Playback(playback) = &client.replay_mode else {
                panic!(
                    "{} should install its stored witness: {:?}",
                    level.id(),
                    client.replay_notice.as_ref().map(|notice| &notice.detail)
                );
            };
            assert!(
                matches!(playback.origin, ReplayOrigin::Gallery(actual) if actual.id() == level.id())
            );
            assert_eq!(
                playback.expected_objective,
                Some(ReplayObjective::Exit(level.target().to_owned()))
            );
            assert_eq!(client.simulation.digest(), playback.replay.initial_digest);
            assert!(playback.replay.actions().all(|action| !action.dash));
            let frame_count = playback.transport.total_frames;
            assert!(frame_count > 0);

            for _ in 0..frame_count {
                client.advance_replay(false);
            }
            assert_eq!(client.simulation.reached_exit(), Some(level.target()));
            assert!(client.replay_complete());
        }
    }

    #[test]
    fn calibrated_v_installs_and_plays_every_mechanically_generated_witness() {
        for level in calibrated_generator_playtest().iter().copied() {
            let selection = ScenarioSelection {
                mode: RoomMode::CalibratedGenerated,
                seed: level.seed(),
                tier: AbilityTier::WallJump,
            };
            let mut client = ClientState::new(selection, None, false).unwrap();
            assert_eq!(client.browser_mode, BrowserMode::Closed);
            client.request_solve();
            client.perform_requested_solve();

            let ReplayMode::Playback(playback) = &client.replay_mode else {
                panic!(
                    "generated seed {} should install its witness: {:?}",
                    level.seed(),
                    client.replay_notice.as_ref().map(|notice| &notice.detail)
                );
            };
            assert!(matches!(
                playback.origin,
                ReplayOrigin::CalibratedGenerated(actual) if actual.seed() == level.seed()
            ));
            assert_eq!(
                playback.expected_objective,
                Some(ReplayObjective::Exit(level.target().to_owned()))
            );
            assert!(playback.replay.actions().all(|action| !action.dash));
            let frame_count = playback.transport.total_frames;
            for _ in 0..frame_count {
                client.advance_replay(false);
            }
            assert_eq!(client.simulation.reached_exit(), Some(level.target()));
            assert!(client.replay_complete());
        }
    }

    #[test]
    fn default_entry_live_solves_its_coin_under_current_movement() {
        let selection = ScenarioSelection::default();
        let mut client = ClientState::new(selection, None, false).unwrap();
        let pickup_id = client.simulation.room().pickups()[0].id().to_owned();

        client.request_pickup_solve();
        assert!(matches!(
            &client.replay_mode,
            ReplayMode::SolveRequested(SolveRequest::Pickup(actual)) if actual == &pickup_id
        ));
        client.perform_requested_solve();

        let ReplayMode::Playback(playback) = &client.replay_mode else {
            panic!(
                "default catalogue entry should produce a coin replay: {:?}",
                client.replay_notice.as_ref().map(|notice| &notice.detail)
            );
        };
        assert_eq!(
            playback.expected_objective,
            Some(ReplayObjective::Pickup(pickup_id.clone()))
        );
        assert!(matches!(
            &playback.origin,
            ReplayOrigin::PickupSolver {
                pickup_id: actual,
                stored_witness: false,
                search_stats: Some(_),
            } if actual == &pickup_id
        ));
        let frame_count = playback.transport.total_frames;
        assert!(frame_count > 0);

        for _ in 0..frame_count {
            client.advance_replay(false);
        }
        assert!(
            client
                .simulation
                .collected_pickups()
                .any(|pickup| pickup.id() == pickup_id)
        );
        assert!(client.replay_complete());
    }

    #[test]
    fn coin_request_explains_rooms_with_no_coin_or_multiple_coins() {
        let mut client = ClientState::new(ScenarioSelection::default(), None, false).unwrap();
        client.selection.mode = RoomMode::Development;
        client.simulation = Simulation::new(test_room_with_pickups(Vec::new()));
        assert_eq!(
            client.pickup_solve_target(),
            Err(PickupSolveRequestError::NoPickups)
        );
        client.request_pickup_solve();
        assert!(matches!(client.replay_mode, ReplayMode::Human));
        let notice = client.replay_notice.as_ref().unwrap();
        assert_eq!(notice.title, "NO COIN IN THIS ROOM");
        assert_ne!(client.simulation.room().pickups().len(), 1);

        client.simulation = Simulation::new(test_room_with_pickups(vec![
            Pickup::new("left", CoreRect::new(40, 148, 6, 8)).unwrap(),
            Pickup::new("right", CoreRect::new(80, 148, 6, 8)).unwrap(),
        ]));
        assert_eq!(
            client.pickup_solve_target(),
            Err(PickupSolveRequestError::MultiplePickups(2))
        );
        client.request_pickup_solve();
        assert!(matches!(client.replay_mode, ReplayMode::Human));
        let notice = client.replay_notice.as_ref().unwrap();
        assert_eq!(notice.title, "CHOOSE A COIN");
        assert!(notice.detail.contains("2 coins"));
        assert_ne!(client.simulation.room().pickups().len(), 1);
    }

    fn test_room_with_pickups(pickups: Vec<Pickup>) -> Room {
        let mut tiles = vec![Tile::Empty; 32 * 18];
        for tile in &mut tiles[16 * 32..17 * 32] {
            *tile = Tile::Solid;
        }
        Room::new(
            "client-test",
            "Client test",
            32,
            18,
            10,
            tiles,
            Point::new(20, 148),
            Vec::new(),
        )
        .unwrap()
        .with_objects(Vec::new(), pickups)
        .unwrap()
    }

    #[test]
    fn died_and_automatic_reset_in_one_report_preserve_death_feedback() {
        let mut feedback = SimulationFeedback::default();
        feedback.observe(&[
            SimulationEvent::Died(DeathReason::Hazard {
                tile_x: 7,
                tile_y: 16,
            }),
            SimulationEvent::Reset,
        ]);
        assert_eq!(feedback.death_ticks, DEATH_FEEDBACK_TICKS);
        assert_eq!(feedback.last_death_reason, "SPIKES");

        feedback.advance_tick();
        assert_eq!(feedback.death_ticks, DEATH_FEEDBACK_TICKS - 1);
        feedback.observe(&[SimulationEvent::Reset]);
        assert_eq!(feedback.death_ticks, 0);
    }

    #[test]
    fn player_pose_cells_cover_the_generated_sheet_without_overlap() {
        let poses = [
            PlayerPose::IdleA,
            PlayerPose::IdleB,
            PlayerPose::RunContact,
            PlayerPose::RunPassing,
            PlayerPose::RunContactOpposite,
            PlayerPose::RunPassingOpposite,
            PlayerPose::Rising,
            PlayerPose::Falling,
            PlayerPose::WallCling,
            PlayerPose::WallJump,
            PlayerPose::Skid,
            PlayerPose::DeepSkid,
        ];
        let sources = poses.map(PlayerPose::sheet_source);
        assert_eq!(sources[0], Rect::new(0.0, 0.0, 24.0, 24.0));
        assert_eq!(sources[11], Rect::new(72.0, 48.0, 24.0, 24.0));
        for (index, source) in sources.iter().enumerate() {
            assert!(source.right() <= PLAYER_SPRITE_SHEET_WIDTH);
            assert!(source.bottom() <= PLAYER_SPRITE_SHEET_HEIGHT);
            assert!(!sources[..index].contains(source));
        }
    }

    #[test]
    fn environment_cells_cover_every_rendered_landscape_feature() {
        let sprites = [
            EnvironmentSprite::SolidA,
            EnvironmentSprite::SolidB,
            EnvironmentSprite::OneWay,
            EnvironmentSprite::SpikesUp,
            EnvironmentSprite::SpikesDown,
            EnvironmentSprite::SpikesHorizontal,
            EnvironmentSprite::Exit,
            EnvironmentSprite::Door,
            EnvironmentSprite::Pickup,
        ];
        let sources = sprites.map(EnvironmentSprite::sheet_source);
        for (index, source) in sources.iter().enumerate() {
            assert!(source.right() <= ENVIRONMENT_SHEET_WIDTH);
            assert!(source.bottom() <= ENVIRONMENT_SHEET_HEIGHT);
            assert!(!sources[..index].contains(source));
        }
    }

    #[test]
    fn dungeon_pickup_cells_are_distinct_and_cover_the_exact_sheet() {
        let sources = [
            DungeonPickupSprite::WingedBoots.sheet_source(),
            DungeonPickupSprite::Crown.sheet_source(),
        ];
        assert_eq!(sources[0], Rect::new(0.0, 0.0, 16.0, 16.0));
        assert_eq!(sources[1], Rect::new(16.0, 0.0, 16.0, 16.0));
        assert!(sources.iter().all(|source| {
            source.right() <= DUNGEON_PICKUP_SHEET_WIDTH
                && source.bottom() <= DUNGEON_PICKUP_SHEET_HEIGHT
        }));
    }

    #[test]
    fn low_clearance_spikes_point_into_the_actual_route_corridors() {
        let scenario = calibration_gallery()[4].scenario();
        let room = scenario.room();
        assert_eq!(room.name(), "Low Clearance");

        assert_eq!(room.hazard_direction(20, 3), Some(HazardDirection::Up));
        assert_eq!(room.hazard_direction(20, 4), Some(HazardDirection::Down));
        assert_eq!(
            spike_base_pair(room, 20, 3),
            Some(SpikeBasePair::Horizontal)
        );
        assert_eq!(room.hazard_direction(20, 12), Some(HazardDirection::Up));
        assert_eq!(room.hazard_direction(11, 11), Some(HazardDirection::Right));
        assert_eq!(room.hazard_direction(15, 13), Some(HazardDirection::Left));
    }

    #[test]
    fn movement_feedback_tracks_skids_wall_jumps_and_resets_as_presentation_only_state() {
        let mut feedback = SimulationFeedback::default();
        feedback.observe_motion(
            Action {
                move_x: -1,
                ..Action::default()
            },
            SUBPIXELS_PER_PIXEL * 2,
            true,
            true,
        );
        assert_eq!(feedback.skid_ticks, SKID_FEEDBACK_TICKS);
        assert_eq!(feedback.skid_direction, 1);

        feedback.observe(&[SimulationEvent::Jumped(JumpKind::Wall {
            side: WallSide::Right,
        })]);
        assert_eq!(feedback.skid_ticks, 0);
        assert_eq!(feedback.wall_jump_ticks, WALL_JUMP_FEEDBACK_TICKS);
        assert_eq!(feedback.wall_jump_side, Some(WallSide::Right));

        feedback.advance_tick();
        assert_eq!(feedback.wall_jump_ticks, WALL_JUMP_FEEDBACK_TICKS - 1);
        feedback.observe(&[SimulationEvent::Reset]);
        assert_eq!(feedback.wall_jump_ticks, 0);
        assert_eq!(feedback.wall_jump_side, None);
        assert_eq!(feedback.landing_ticks, 0);
    }

    #[test]
    fn subpixel_display_is_compact_and_signed() {
        assert_eq!(format_subpixels(384), "1.5");
        assert_eq!(format_subpixels(-384), "-1.5");
        assert_eq!(format_subpixels(0), "0.0");
    }

    #[test]
    fn between_frame_jump_tap_emits_one_tick_for_immediate_jump_kinds() {
        for kind in [
            downwards_core::JumpKind::Grounded,
            downwards_core::JumpKind::Coyote,
            downwards_core::JumpKind::Wall {
                side: WallSide::Left,
            },
        ] {
            let mut input = HumanJumpInput::default();
            input.observe_frame(true, true, false, 0, false);
            assert!(input.action_for_tick());
            input.observe_simulation_step(&[SimulationEvent::Jumped(kind)]);
            assert!(!input.action_for_tick());
        }
    }

    #[test]
    fn ordinary_wall_clock_tap_durations_select_the_same_low_gesture() {
        for duration_micros in [1_000, 25_000, 50_000, 75_000, 99_999] {
            let mut input = HumanJumpInput::default();
            input.observe_frame(true, false, true, 10, false);

            // Render frames and fixed updates may occur while the physical key is down, but a
            // not-yet-classified gesture must not leak variable held durations into physics.
            input.observe_frame(false, false, true, 10 + duration_micros / 2, false);
            assert!(!input.action_for_tick());
            assert!(!input.action_for_tick());

            input.observe_frame(false, true, false, 10 + duration_micros, false);
            assert!(input.action_for_tick());
            input.observe_simulation_step(&[SimulationEvent::Jumped(
                downwards_core::JumpKind::Grounded,
            )]);
            assert!(!input.action_for_tick());
            assert!(!input.action_for_tick());
        }
    }

    #[test]
    fn wall_jump_press_is_immediate_and_release_does_not_wait_for_tap_classification() {
        let mut input = HumanJumpInput::default();
        input.observe_frame(true, false, true, 10, true);
        assert!(input.action_for_tick());
        input.observe_simulation_step(&[SimulationEvent::Jumped(downwards_core::JumpKind::Wall {
            side: WallSide::Left,
        })]);

        input.observe_frame(false, true, false, 25_000, true);
        assert!(!input.action_for_tick());
    }

    #[test]
    fn held_jump_remains_variable_after_the_minimum_tap_window() {
        let mut input = HumanJumpInput::default();
        input.observe_frame(true, false, true, 0, false);
        assert!(!input.action_for_tick());
        input.observe_frame(false, false, true, HUMAN_JUMP_TAP_WINDOW_MICROS, false);
        assert!(input.action_for_tick());
        input.observe_simulation_step(&[SimulationEvent::Jumped(
            downwards_core::JumpKind::Grounded,
        )]);
        input.observe_frame(
            false,
            false,
            true,
            HUMAN_JUMP_TAP_WINDOW_MICROS + 10_000,
            false,
        );
        assert!(input.action_for_tick());
        input.observe_frame(
            false,
            false,
            true,
            HUMAN_JUMP_TAP_WINDOW_MICROS + 20_000,
            false,
        );
        assert!(input.action_for_tick());

        input.observe_frame(
            false,
            true,
            false,
            HUMAN_JUMP_TAP_WINDOW_MICROS + 30_000,
            false,
        );
        assert!(!input.action_for_tick());
    }

    #[test]
    fn release_then_repress_is_not_lost_when_no_simulation_tick_intervenes() {
        let mut input = HumanJumpInput::default();
        input.observe_frame(true, false, true, 0, false);
        input.observe_frame(false, false, true, HUMAN_JUMP_TAP_WINDOW_MICROS, false);
        assert!(input.action_for_tick());
        input.observe_simulation_step(&[SimulationEvent::Jumped(
            downwards_core::JumpKind::Grounded,
        )]);
        assert!(input.action_for_tick());

        input.observe_frame(false, true, false, HUMAN_JUMP_TAP_WINDOW_MICROS + 1, false);
        input.observe_frame(true, false, true, HUMAN_JUMP_TAP_WINDOW_MICROS + 2, false);
        assert!(!input.action_for_tick());
        input.observe_frame(
            false,
            false,
            true,
            HUMAN_JUMP_TAP_WINDOW_MICROS * 2 + 2,
            false,
        );
        assert!(input.action_for_tick());
    }

    #[test]
    fn releasing_one_jump_alias_does_not_cut_another_held_alias() {
        let mut input = HumanJumpInput::default();
        input.observe_frame(true, false, true, 0, false);
        input.observe_frame(false, false, true, HUMAN_JUMP_TAP_WINDOW_MICROS, false);
        assert!(input.action_for_tick());
        input.observe_simulation_step(&[SimulationEvent::Jumped(
            downwards_core::JumpKind::Grounded,
        )]);
        assert!(input.action_for_tick());

        // For example, Z was released while Space remains held. The aggregate held state is the
        // authoritative human intent, so no false edge may reach the simulation.
        input.observe_frame(false, true, true, HUMAN_JUMP_TAP_WINDOW_MICROS + 1, false);
        assert!(input.action_for_tick());
        assert!(input.action_for_tick());

        // A complete tap of a secondary alias inside one frame is likewise invisible while the
        // primary alias remains continuously held.
        input.observe_frame(true, true, true, HUMAN_JUMP_TAP_WINDOW_MICROS + 2, false);
        assert!(input.action_for_tick());
        assert!(input.action_for_tick());
    }

    #[test]
    fn released_tap_is_one_semantic_press_even_while_the_core_buffers_it() {
        let mut input = HumanJumpInput::default();
        input.observe_frame(true, true, false, 0, false);
        assert!(input.action_for_tick());
        assert!(input.action_for_tick());
        assert!(input.action_for_tick());

        // Acceptance may happen later. The adapter retains one semantic press until then, then
        // releases on the following simulation tick so wall-clock tap duration cannot inflate it.
        input.observe_simulation_step(&[SimulationEvent::Jumped(
            downwards_core::JumpKind::Buffered,
        )]);
        assert!(!input.action_for_tick());
    }

    fn human_input_jump_rise(physical_hold_ticks: Option<usize>) -> i32 {
        let mut simulation = Simulation::new(test_room_with_pickups(Vec::new()));
        simulation.step(Action::default());
        assert!(simulation.player().grounded());
        let start_y = simulation.player().position_subpixels().y;
        let mut minimum_y = start_y;
        let mut input = HumanJumpInput::default();
        match physical_hold_ticks {
            Some(_) => {
                input.observe_frame(true, false, true, 0, false);
                input.observe_frame(false, false, true, HUMAN_JUMP_TAP_WINDOW_MICROS, false);
            }
            None => input.observe_frame(true, true, false, 0, false),
        }
        let mut jumped = false;

        for tick in 0..120 {
            if physical_hold_ticks == Some(tick) {
                input.observe_frame(
                    false,
                    true,
                    false,
                    HUMAN_JUMP_TAP_WINDOW_MICROS + tick as u64 * 16_667,
                    false,
                );
            }
            let report = simulation.step(Action {
                jump: input.action_for_tick(),
                ..Action::default()
            });
            input.observe_simulation_step(&report.events);
            jumped |= report
                .events
                .iter()
                .any(|event| matches!(event, SimulationEvent::Jumped(_)));
            minimum_y = minimum_y.min(simulation.player().position_subpixels().y);
            if jumped && simulation.player().velocity_subpixels().y >= 0 {
                return start_y - minimum_y;
            }
        }
        panic!("human jump did not reach its apex within the test horizon");
    }

    #[test]
    fn human_tap_and_hold_produce_distinct_stable_jump_heights() {
        assert_eq!(human_input_jump_rise(None), 1_744);
        assert_eq!(human_input_jump_rise(Some(10)), 7_792);
    }

    #[test]
    fn dungeon_transitions_persist_both_unlocks_and_follow_the_authored_loop_to_the_crown() {
        let selection = ScenarioSelection {
            mode: RoomMode::Dungeon,
            seed: 0,
            tier: AbilityTier::Baseline,
        };
        let mut client = ClientState::new(selection, None, false).unwrap();
        assert_eq!(client.dungeon_run, Some(DungeonRunState::default()));
        assert!(!client.simulation.abilities().dash);

        let report = |client: &ClientState, events: Vec<SimulationEvent>| StepReport {
            tick: client.simulation.tick(),
            events,
            digest: client.simulation.digest(),
        };
        let exit = |id: &str| SimulationEvent::ExitReached { id: id.to_owned() };
        let coins = |indices: &[u8]| {
            indices
                .iter()
                .map(|index| SimulationEvent::PickupCollected {
                    id: format!("dungeon-coin-{index:02}"),
                })
                .collect::<Vec<_>>()
        };

        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[0]))));

        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::MossWalk);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[1]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::SplitRoot);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::RootCellar
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[2, 3]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::SplitRoot);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::BrokenAqueduct
        );
        // Coin 4 is deliberately skipped: gate tolerance means progression
        // never needs every coin, and the Sluice gate below still seals at 4.
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::OldLift);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::LanternGallery
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Sluice);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Sluice);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("west")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::WatchPost);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[5]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::LanternGallery
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::ClimberVault
        );
        assert!(client.observe_dungeon_progress(&report(
            &client,
            vec![SimulationEvent::PickupCollected {
                id: DEMO_DUNGEON_GLOVE_PICKUP.to_owned(),
            }],
        )));
        assert!(client.dungeon_run.unwrap().inventory.climbing_gloves);
        assert!(client.simulation.abilities().wall_jump);
        assert!(client.observe_dungeon_progress(&report(&client, vec![SimulationEvent::Reset])));
        assert!(
            client
                .simulation
                .room()
                .pickups()
                .iter()
                .all(|pickup| pickup.id() != DEMO_DUNGEON_GLOVE_PICKUP)
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::WallAntechamber
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[16]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::BroadChimney
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[17]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::BellSwitchback
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::BellNiche);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[18]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::BellSwitchback
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::TempoHall);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[19]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::SplitSpire
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::RafterShrine
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[20]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::SplitSpire
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::LandingChain
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::NeedleTurn
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::WallGate);
        // With coins 4 and 21 skipped the run sits exactly at the wall gate's
        // tolerant requirement, so the seal opens without a completionist
        // backtrack.
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 10);
        assert_eq!(
            downwards_content::DEMO_DUNGEON_WALL_REGION_GATE_REQUIREMENT,
            10
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Threshold);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[6]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::Crossroads
        );
        assert_eq!(client.simulation.entry_door(), Some("west"));
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[7]))));

        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::Crossroads
        );

        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::CoinLoft);
        assert_eq!(client.simulation.entry_door(), Some("ceiling"));
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[8, 9]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::Crossroads
        );
        assert_eq!(client.simulation.entry_door(), Some("floor"));

        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::WallGallery
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[10]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::NeedleRoom
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[11]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::WallGallery
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("west")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::Crossroads
        );

        // Eighteen coins expose the lower vault route, but no longer skip its Underpass and
        // Treasury trials by opening the Winged Vault directly.
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::Crossroads
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::WallGallery
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Underpass);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[13]))));
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 17);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("west")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Underpass);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Treasury);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[14, 15]))));
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 19);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("west")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Underpass);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("west")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::BootsVault
        );
        assert_eq!(client.simulation.entry_door(), Some("east"));

        assert!(client.observe_dungeon_progress(&report(
            &client,
            vec![
                SimulationEvent::PickupCollected {
                    id: "dungeon-coin-12".to_owned(),
                },
                SimulationEvent::PickupCollected {
                    id: DEMO_DUNGEON_BOOT_PICKUP.to_owned(),
                },
            ],
        )));
        assert!(client.dungeon_run.unwrap().inventory.winged_boots);
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 20);
        assert!(client.simulation.abilities().dash);
        assert!(client.simulation.player().dash_available());
        assert!(client.observe_dungeon_progress(&report(&client, vec![SimulationEvent::Reset],)));
        assert!(client.simulation.room().pickups().is_empty());
        assert!(client.simulation.abilities().dash);

        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Underpass);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::WallGallery
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::DashChasm);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::GaleLanding
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("west")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::DashChasm);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("west")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::WallGallery
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::GaleLanding
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[22]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::LowPassage
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[23]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::CurrentFork
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::CoinDuct);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[24]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::CurrentFork
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::PulseGallery
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[25]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::StormSplit
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::StormCache
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[26]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::StormSplit
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::RelayChasm
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::BrakeTower
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::DashSeal);
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 25);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[27]))));
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 26);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::AlloyThreshold
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[28]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Windshaft);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[29]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::SplitFurnace
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::EmberVault
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[30]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::GearGallery
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[31]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::CrosswindChimney
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::FoundryFork
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::CoolingDuct
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[32]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::HammerHall
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[33]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::LiftShaft);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::SparkNiche
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[34]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::RivetRun);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::BlastGallery
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[35]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::PressureFork
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::AshCache);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[36]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::VentSpire);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[37]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::PistonPass
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::CrucibleClimb
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::CinderBridge
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[38]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::FoundrySeal
        );
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 37);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[39]))));
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 38);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::GlassThreshold
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[40]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::PrismRun);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[41]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::SplitKiln);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::ShardVault
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[42]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::GlassGallery
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[43]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::RefractionShaft
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::MirrorFork
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::MirrorDuct
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[44]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::TemperHall
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[45]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::FurnaceLift
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::LensNiche);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[46]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::SliverRun);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::HotGlass);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[47]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::CulletFork
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::CulletCache
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[48]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::AnnealingSpire
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[49]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::RazorPass);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::LatticeClimb
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::CrystalBridge
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[50]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::GlassSeal);
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 49);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[51]))));
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 50);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::StarThreshold
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[52]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::CometRun);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[53]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::OrbitFork);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::MoonVault);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[54]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::ConstellationHall
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[55]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::ZenithShaft
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::EclipseFork
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::ShadowDuct
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[56]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::Observatory
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[57]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::GravityLift
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::NovaNiche);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[58]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::MeteorRun);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::VacuumGallery
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[59]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::TidalFork);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("floor")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::LunarCache
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[60]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("ceiling")])));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::AuroraSpire
        );
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[61]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::VoidPass);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::StarwellClimb
        );
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Skybridge);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[62]))));
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::AstralSeal
        );
        // Two coins were skipped along the run; the tolerant astral and crown
        // gates open regardless, and its own coin is not required to leave.
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 61);
        assert!(!client.observe_dungeon_progress(&report(&client, coins(&[63]))));
        assert_eq!(client.dungeon_run.unwrap().inventory.coin_count(), 62);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(client.dungeon_run.unwrap().room, DemoDungeonRoom::Gatehouse);
        assert!(client.observe_dungeon_progress(&report(&client, vec![exit("east")])));
        assert_eq!(
            client.dungeon_run.unwrap().room,
            DemoDungeonRoom::CrownSanctum
        );
        assert_eq!(client.simulation.entry_door(), Some("west"));

        assert!(!client.observe_dungeon_progress(&report(
            &client,
            vec![
                SimulationEvent::PickupCollected {
                    id: DEMO_DUNGEON_CROWN_PICKUP.to_owned(),
                },
                exit(DEMO_DUNGEON_GOAL_EXIT),
            ],
        )));
        assert!(client.dungeon_run.unwrap().inventory.crown);
        assert_eq!(client.selected_target_id(), Some(DEMO_DUNGEON_GOAL_EXIT));
    }

    #[test]
    fn reset_events_clear_tap_state_before_the_fresh_attempt() {
        for events in [
            vec![SimulationEvent::Reset],
            vec![
                SimulationEvent::Jumped(downwards_core::JumpKind::Grounded),
                SimulationEvent::Died(DeathReason::Hazard {
                    tile_x: 4,
                    tile_y: 7,
                }),
                SimulationEvent::Reset,
            ],
        ] {
            let mut input = HumanJumpInput::default();
            input.observe_frame(true, true, false, 0, false);
            assert!(input.action_for_tick());
            input.observe_simulation_step(&events);
            assert!(!input.action_for_tick());
        }
    }

    #[test]
    fn controls_explain_variable_jump_height_in_plain_language() {
        assert!(USAGE.contains("tap or release early for low, hold for high"));
        for tier in [
            AbilityTier::Baseline,
            AbilityTier::WallJump,
            AbilityTier::Dash,
            AbilityTier::WallJumpAndDash,
        ] {
            assert!(gameplay_control_summary(tier).contains("TAP=LOW HOLD=HIGH"));
        }
    }

    #[test]
    fn win_stats_line_is_bounded_for_saturated_session_counters() {
        let oversized = format!(
            "RUNS {}  DEATHS {}  CLEARS {}  COINS {}  BEST {}",
            u32::MAX,
            u32::MAX,
            u32::MAX,
            u32::MAX,
            format_best_clear(Some(usize::MAX))
        );
        let fitted = fit_win_line(&oversized);
        assert_eq!(fitted.chars().count(), 80);
        assert!(fitted.ends_with("..."));
    }

    #[test]
    fn provisional_diagnostics_format_robustness_without_overclaiming() {
        assert_eq!(
            complexity_band_label(ComplexityBand::Technical),
            "TECHNICAL"
        );
        assert_eq!(format_robustness(Some(0.483)), "48%");
        assert_eq!(format_robustness(None), "N/A");
    }

    #[test]
    fn viewport_uses_largest_integer_scale_and_centres_the_room_plus_hud_canvas() {
        let viewport = PixelViewport::for_window(1_000.0, 700.0);
        assert_eq!(
            viewport,
            PixelViewport {
                scale: 3.0,
                left: 20.0,
                top: 50.0
            }
        );
        assert_eq!(viewport.translated(0, ROOM_TOP).top, 80.0);
    }

    #[test]
    fn viewport_letterboxes_a_different_aspect_ratio() {
        let viewport = PixelViewport::for_window(640.0, 480.0);
        assert_eq!(
            viewport,
            PixelViewport {
                scale: 2.0,
                left: 0.0,
                top: 40.0
            }
        );
    }
}
