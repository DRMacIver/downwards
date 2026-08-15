# ADR 0001: Macroquad, mlua, and a renderer-free simulation core

- Status: Superseded in part by [ADR 0003](0003-rust-owned-content.md)
- Date: 2026-08-14

ADR 0003 removes the embedded scripting and non-Rust room-authoring portions of this decision.
The Macroquad choice and renderer-free deterministic simulation boundary remain in force. The
text below is retained as the historical decision record.

## Context

Downwards is a fixed-screen, pixel-art platformer whose early development is
focused on movement mechanics, traps, room generation, and automated level
evaluation. The same level must be playable by a person and executable much
faster than real time by search and testing tools. It also needs Lua for
authoring content and scripted mechanisms.

These needs make the boundary between simulation and presentation more
important than the choice of renderer. A window, GPU, audio device, wall clock,
or human-input API must not be required to construct or advance a game state.

## Decision

Use [Macroquad 0.4.15](https://docs.rs/macroquad/0.4.15/macroquad/) for the
desktop client and [mlua 0.12](https://docs.rs/mlua/0.12.0/mlua/) with the
`lua55` and `vendored` features for embedded Lua. The vendored mlua dependency
selects the Lua 5.5.1 line through
[`lua-src`](https://docs.rs/crate/mlua-sys/0.11.0/source/Cargo.toml.orig), so a
system Lua installation is not required.

Keep the following dependency boundary:

```text
downwards-core    deterministic state, rules, physics, collision, and seeded RNG
       ^
       +-- downwards-ai       solver and difficulty analysis
       +-- downwards-gen      seeded room candidates
       +-- downwards-lua      Lua host and conversion to validated core data
       +-- downwards-catalogue  strict loading of offline-curated room manifests
       +-- downwards-client   Macroquad input, rendering, and audio
       +-- downwards-tools    headless generation and evaluation commands
       +-- downwards-validation  scenario objectives and acceptance certificates
```

`downwards-core` must not depend on Macroquad, mlua, windowing, rendering,
audio, or wall-clock time. It exposes an explicit fixed-timestep operation such
as `Simulation::step(Action)`. The client translates device input into actions
and renders resulting state; the AI supplies actions to that same operation.
Tests and tools can therefore run simulations without initializing Macroquad or
creating a window.

Lua is behind the same boundary. Scripts should initially produce declarative
room definitions, generator rules, trap parameters, and event-driven mechanism
definitions. `downwards-lua` validates and converts those values into types
owned by `downwards-core`. Authoritative movement, collision, topology
constraints, and randomness remain in Rust. Lua must receive seeded game RNG
through the host rather than using wall time or an uncontrolled random source.

Macroquad fits the presentation layer because it is a small, immediate-mode 2D
library with official desktop support for macOS, Windows, and Linux. Its
[render-target API](https://docs.rs/macroquad/0.4.15/macroquad/texture/fn.render_target.html)
supports rendering to a low-resolution logical canvas and applying nearest
filtering for pixel-perfect scaling. Macroquad does not document a headless
runner; that is why headless execution belongs to `downwards-core`, not to a
special client mode.

## Alternatives considered

### Bevy 0.19

[Bevy 0.19](https://bevy.org/news/bevy-0-19/) has good native support for a
[minimal headless application](https://docs.rs/bevy/0.19.0/bevy/struct.MinimalPlugins.html)
and [manually controlled time in tests](https://docs.rs/bevy/0.19.0/bevy/time/enum.TimeUpdateStrategy.html).
It also provides much more ECS, renderer, asset, and application machinery than
the initial one-screen mechanics prototype needs. We prefer Macroquad's smaller
surface while retaining a framework-independent core.

Bevy's scripting ecosystem is also a reason not to couple Lua to the engine at
this stage. The current
[`bevy_mod_scripting` documentation](https://docs.rs/bevy_mod_scripting/0.20.0/bevy_mod_scripting/)
describes the project as incomplete and work in progress, and its compatibility
table maps its current release to Bevy 0.18 rather than Bevy 0.19. Direct mlua
keeps the script interface under our control.

Reconsider Bevy if the project later needs its editor-oriented ecosystem,
large-scale ECS facilities, or integrated controller support enough to justify
the additional engine coupling.

### ggez 0.10

[ggez 0.10](https://docs.rs/ggez/0.10.0/ggez/) has an appealing Love2D-style 2D
API, but its official documentation says macOS is not officially supported. As
macOS is a primary development platform for this project, that makes it a poor
default. Its [`Context`](https://docs.rs/ggez/0.10.0/ggez/context/struct.Context.html)
also groups graphics, audio, input, and other hardware-facing state, which does
not improve the required headless boundary.

## Platform and input caveats

The project is intentionally desktop- and keyboard-first. Browser support is deliberately not
promised by this decision: Macroquad's documented web build uses
[`wasm32-unknown-unknown`](https://docs.rs/crate/macroquad/0.4.15), while mlua
documents Lua WebAssembly support through
[`wasm32-unknown-emscripten`](https://github.com/mlua-rs/mlua#webassembly).
Before committing to a browser release, build a small compatibility spike that
loads a real script and runs the simulation in the intended browser toolchain.
The core boundary makes replacing either adapter possible if that spike fails.

Keyboard input is sufficient for the initial mechanics work. Macroquad's
current [input module](https://docs.rs/macroquad/0.4.15/macroquad/input/) still
describes gamepad support as forthcoming. Before controller support becomes a
deliverable, evaluate a separate gamepad adapter on every desktop target or
revisit the client framework choice. Device-specific input must still be
converted into core `Action` values so recordings, AI, and tests remain
independent of the adapter.

## Consequences

- Human play, replay, AI search, and property tests share one simulation path.
- Simulation tests remain fast and do not need a display or GPU.
- Presentation and scripting libraries can be upgraded or replaced without
  rewriting the game rules.
- We must build and maintain our own narrow asset, scripting, input, and
  simulation interfaces instead of relying on a full engine's object model.
- Web and polished controller support remain explicit follow-up decisions, not
  accidental claims of the initial stack.
