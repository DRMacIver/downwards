//! Strict, compact runtime bridge from the verified room corpus to the game.
//!
//! The manifest stores native regeneration coordinates and a replay-certified
//! representative route. It never serializes or reconstructs a flattened room.

use std::{collections::HashSet, error::Error, fmt};

use downwards_ai::{Replay, SearchStats};
use downwards_core::{AbilitySet, Action, BoundarySide, DoorSocket, Simulation};
use downwards_gen::{
    GeneratedLevel,
    experimental::{
        COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
        COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
        COMPOSITIONAL_ABILITY_GENERATION_VERSION, COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
        COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
        COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION, ChallengeIntent,
        CompositionalAbilityEdgeRewriteKey, CompositionalAbilityGateProfile,
        CompositionalAbilityGenerationKey, CompositionalRouteCutGrammar, CompositionalRouteCutKey,
        PARTITION_ROUTE_GENERATION_VERSION, PartitionRouteKey, PartitionRouteProfile,
    },
};
use downwards_validation::WITNESS_FINGERPRINT_VERSION;

use super::{ActionSpan, RepresentativeActions};

pub const CORPUS_PLAYTEST_MANIFEST_VERSION: u32 = 1;
pub const CORPUS_PLAYTEST_KEY_RECORD_VERSION: u32 = 2;

const HEADER: &str = "downwards-corpus-playtest-v1";
const FINGERPRINT_PREFIX: &str = "manifest-fingerprint=downwards-corpus-playtest-v1-";
const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 1_024;
const MAX_ACTION_SPANS: usize = 4_096;
const MAX_REPLAY_TICKS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusPlaytestPublicationState {
    FinalRecomputed,
    ProvisionalOperationalCache,
}

impl CorpusPlaytestPublicationState {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::FinalRecomputed => "final-recomputed",
            Self::ProvisionalOperationalCache => "provisional-operational-cache",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusPlaytestRouteSelection {
    AuthoredSourceSink,
    DeterministicPositiveFallback,
}

impl CorpusPlaytestRouteSelection {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::AuthoredSourceSink => "authored-source-sink",
            Self::DeterministicPositiveFallback => "deterministic-positive-fallback",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusPlaytestLoadout {
    Baseline,
    WallJump,
    Dash,
    Both,
}

impl CorpusPlaytestLoadout {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::WallJump => "wall-jump",
            Self::Dash => "dash",
            Self::Both => "both",
        }
    }

    pub const fn abilities(self) -> AbilitySet {
        match self {
            Self::Baseline => AbilitySet::NONE,
            Self::WallJump => AbilitySet::new(true, false),
            Self::Dash => AbilitySet::new(false, true),
            Self::Both => AbilitySet::ALL,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusPlaytestIntent {
    Gentle,
    Standard,
    Technical,
}

impl CorpusPlaytestIntent {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Gentle => "gentle",
            Self::Standard => "standard",
            Self::Technical => "technical",
        }
    }

    const fn native(self) -> ChallengeIntent {
        match self {
            Self::Gentle => ChallengeIntent::Gentle,
            Self::Standard => ChallengeIntent::Standard,
            Self::Technical => ChallengeIntent::Technical,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusPlaytestPartitionProfile {
    MixedBsp,
    Columnar,
    Branching,
}

impl CorpusPlaytestPartitionProfile {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::MixedBsp => "mixed-bsp",
            Self::Columnar => "columnar",
            Self::Branching => "branching",
        }
    }

    const fn native(self) -> PartitionRouteProfile {
        match self {
            Self::MixedBsp => PartitionRouteProfile::MixedBsp,
            Self::Columnar => PartitionRouteProfile::Columnar,
            Self::Branching => PartitionRouteProfile::Branching,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusPlaytestAbilityProfile {
    WallJump,
    Dash,
}

impl CorpusPlaytestAbilityProfile {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::WallJump => "wall-jump",
            Self::Dash => "dash",
        }
    }

    const fn native(self) -> CompositionalAbilityGateProfile {
        match self {
            Self::WallJump => CompositionalAbilityGateProfile::WallJump,
            Self::Dash => CompositionalAbilityGateProfile::Dash,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorpusPlaytestRouteCutGrammar {
    RecursiveMissionCutsV1,
}

impl CorpusPlaytestRouteCutGrammar {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::RecursiveMissionCutsV1 => "recursive-mission-cuts-v1",
        }
    }

    const fn native(self) -> CompositionalRouteCutGrammar {
        match self {
            Self::RecursiveMissionCutsV1 => CompositionalRouteCutGrammar::RecursiveMissionCutsV1,
        }
    }
}

/// Full native regeneration identity. All version fields are checked before
/// dispatch, and dispatch performs exactly one requested generation attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorpusPlaytestKey {
    PartitionRoute {
        record_version: u32,
        generator_version: u32,
        source_seed: u64,
        loadout: CorpusPlaytestLoadout,
        intent: CorpusPlaytestIntent,
        profile: CorpusPlaytestPartitionProfile,
        embedding_attempt: u8,
    },
    CompositionalRouteCut {
        record_version: u32,
        derivation_version: u32,
        generator_version: u32,
        socket_inventory_version: u32,
        source_seed: u64,
        loadout: CorpusPlaytestLoadout,
        intent: CorpusPlaytestIntent,
        grammar: CorpusPlaytestRouteCutGrammar,
        embedding_attempt: u8,
    },
    CompositionalAbility {
        record_version: u32,
        generation_version: u32,
        edge_rewrite_version: u32,
        gate_embedding_contract_version: u32,
        base_record_version: u32,
        base_derivation_version: u32,
        base_generator_version: u32,
        base_socket_inventory_version: u32,
        source_seed: u64,
        base_loadout: CorpusPlaytestLoadout,
        intent: CorpusPlaytestIntent,
        base_grammar: CorpusPlaytestRouteCutGrammar,
        profile: CorpusPlaytestAbilityProfile,
        embedding_attempt: u8,
        rewrite_attempt: u16,
    },
}

impl CorpusPlaytestKey {
    pub const fn generator_slug(&self) -> &'static str {
        match self {
            Self::PartitionRoute { .. } => "partition-route",
            Self::CompositionalRouteCut { .. } => "compositional-route-cut",
            Self::CompositionalAbility { .. } => "compositional-ability",
        }
    }

    pub const fn source_seed(&self) -> u64 {
        match self {
            Self::PartitionRoute { source_seed, .. }
            | Self::CompositionalRouteCut { source_seed, .. }
            | Self::CompositionalAbility { source_seed, .. } => *source_seed,
        }
    }

    pub const fn construction_loadout(&self) -> CorpusPlaytestLoadout {
        match self {
            Self::PartitionRoute { loadout, .. } | Self::CompositionalRouteCut { loadout, .. } => {
                *loadout
            }
            Self::CompositionalAbility { profile, .. } => match profile {
                CorpusPlaytestAbilityProfile::WallJump => CorpusPlaytestLoadout::WallJump,
                CorpusPlaytestAbilityProfile::Dash => CorpusPlaytestLoadout::Dash,
            },
        }
    }

    pub fn regenerate(&self) -> Result<(GeneratedLevel, Vec<DoorSocket>), CorpusPlaytestError> {
        self.validate_versions()?;
        match *self {
            Self::PartitionRoute {
                source_seed,
                loadout,
                intent,
                profile,
                embedding_attempt,
                ..
            } => {
                let candidate = PartitionRouteKey::new(
                    source_seed,
                    loadout.abilities(),
                    intent.native(),
                    profile.native(),
                )
                .with_embedding_attempt(embedding_attempt)
                .regenerate()
                .map_err(|error| {
                    invalid(format!("partition-route regeneration failed: {error}"))
                })?;
                let sockets = candidate
                    .boundary_ports
                    .iter()
                    .map(|port| port.door.socket())
                    .collect();
                Ok((candidate.generated, sockets))
            }
            Self::CompositionalRouteCut {
                source_seed,
                loadout,
                intent,
                grammar,
                embedding_attempt,
                ..
            } => {
                let candidate = CompositionalRouteCutKey::new(
                    source_seed,
                    loadout.abilities(),
                    intent.native(),
                )
                .with_embedding(grammar.native(), embedding_attempt)
                .regenerate()
                .map_err(|error| invalid(format!("route-cut regeneration failed: {error}")))?;
                let sockets = candidate
                    .boundary_ports
                    .iter()
                    .map(|port| port.door.socket())
                    .collect();
                Ok((candidate.generated, sockets))
            }
            Self::CompositionalAbility {
                source_seed,
                base_loadout,
                intent,
                base_grammar,
                profile,
                embedding_attempt,
                rewrite_attempt,
                ..
            } => {
                let base_key = CompositionalRouteCutKey::new(
                    source_seed,
                    base_loadout.abilities(),
                    intent.native(),
                )
                .with_embedding(base_grammar.native(), embedding_attempt);
                let key = CompositionalAbilityGenerationKey {
                    rewrite_key: CompositionalAbilityEdgeRewriteKey::new(
                        base_key,
                        profile.native(),
                    )
                    .with_rewrite_attempt(rewrite_attempt),
                };
                let candidate = key
                    .generate()
                    .map_err(|error| invalid(format!("ability regeneration failed: {error}")))?;
                let sockets = candidate
                    .boundary_ports
                    .iter()
                    .map(|port| port.door.socket())
                    .collect();
                Ok((candidate.generated, sockets))
            }
        }
    }

    fn validate_versions(&self) -> Result<(), CorpusPlaytestError> {
        let ok = match *self {
            Self::PartitionRoute {
                record_version,
                generator_version,
                ..
            } => {
                record_version == CORPUS_PLAYTEST_KEY_RECORD_VERSION
                    && generator_version == PARTITION_ROUTE_GENERATION_VERSION
            }
            Self::CompositionalRouteCut {
                record_version,
                derivation_version,
                generator_version,
                socket_inventory_version,
                ..
            } => {
                record_version == CORPUS_PLAYTEST_KEY_RECORD_VERSION
                    && derivation_version == COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION
                    && generator_version == COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION
                    && socket_inventory_version == COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION
            }
            Self::CompositionalAbility {
                record_version,
                generation_version,
                edge_rewrite_version,
                gate_embedding_contract_version,
                base_record_version,
                base_derivation_version,
                base_generator_version,
                base_socket_inventory_version,
                ..
            } => {
                record_version == CORPUS_PLAYTEST_KEY_RECORD_VERSION
                    && generation_version == COMPOSITIONAL_ABILITY_GENERATION_VERSION
                    && edge_rewrite_version == COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION
                    && gate_embedding_contract_version
                        == COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION
                    && base_record_version == CORPUS_PLAYTEST_KEY_RECORD_VERSION
                    && base_derivation_version == COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION
                    && base_generator_version == COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION
                    && base_socket_inventory_version
                        == COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION
            }
        };
        ok.then_some(())
            .ok_or_else(|| invalid("corpus playtest key uses unsupported mapping versions"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusPlaytestEntry {
    index: usize,
    id: String,
    key: CorpusPlaytestKey,
    route_selection: CorpusPlaytestRouteSelection,
    source_door_id: String,
    target_door_id: String,
    witness_fingerprint: String,
    runtime_replay_checksum: String,
    search_stats: SearchStats,
    initial_digest: u64,
    sockets: Vec<DoorSocket>,
    actions: RepresentativeActions,
}

impl CorpusPlaytestEntry {
    pub const fn index(&self) -> usize {
        self.index
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub const fn key(&self) -> &CorpusPlaytestKey {
        &self.key
    }
    pub const fn route_selection(&self) -> CorpusPlaytestRouteSelection {
        self.route_selection
    }
    pub fn source_door_id(&self) -> &str {
        &self.source_door_id
    }
    pub fn target_door_id(&self) -> &str {
        &self.target_door_id
    }
    pub fn witness_fingerprint(&self) -> &str {
        &self.witness_fingerprint
    }
    pub fn sockets(&self) -> &[DoorSocket] {
        &self.sockets
    }
    pub const fn representative_actions(&self) -> &RepresentativeActions {
        &self.actions
    }

    /// Regenerate the native candidate, check its exact sockets and entry
    /// state, then replay the exported positive through authoritative physics.
    pub fn load_verified(&self) -> Result<Simulation, CorpusPlaytestError> {
        let (generated, sockets) = self.key.regenerate()?;
        if sockets != self.sockets {
            return Err(invalid(format!(
                "room {:?} regenerated with different sockets",
                self.id
            )));
        }
        let initial = Simulation::enter_via_door(
            generated.room.clone(),
            self.key.construction_loadout().abilities(),
            &self.source_door_id,
        )
        .map_err(|error| invalid(format!("room {:?} source door failed: {error}", self.id)))?;
        if initial.digest().0 != self.initial_digest {
            return Err(invalid(format!(
                "room {:?} initial simulation digest changed",
                self.id
            )));
        }
        let replay = Replay::record(&initial, self.actions.actions());
        let verified = replay.verify(&initial).map_err(|error| {
            invalid(format!(
                "room {:?} representative replay changed: {error}",
                self.id
            ))
        })?;
        if verified.reached_exit.as_deref() != Some(self.target_door_id()) {
            return Err(invalid(format!(
                "room {:?} representative replay no longer reaches {:?}",
                self.id, self.target_door_id
            )));
        }
        if corpus_playtest_replay_checksum(
            &self.source_door_id,
            &self.target_door_id,
            self.key.construction_loadout(),
            &replay,
        ) != self.runtime_replay_checksum
        {
            return Err(invalid(format!(
                "room {:?} representative runtime replay checksum changed",
                self.id
            )));
        }
        Ok(initial)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusPlaytestManifest {
    publication_state: CorpusPlaytestPublicationState,
    source_selection_hash: String,
    requested_rooms: usize,
    entries: Vec<CorpusPlaytestEntry>,
}

/// Construction input used by the trusted offline exporter. Runtime callers
/// should parse the rendered manifest rather than constructing entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusPlaytestEntryInput {
    pub id: String,
    pub key: CorpusPlaytestKey,
    pub route_selection: CorpusPlaytestRouteSelection,
    pub source_door_id: String,
    pub target_door_id: String,
    pub witness_fingerprint: String,
    pub runtime_replay_checksum: String,
    pub initial_digest: u64,
    pub search_stats: SearchStats,
    pub sockets: Vec<DoorSocket>,
    pub action_spans: Vec<ActionSpan>,
}

impl CorpusPlaytestManifest {
    pub const fn publication_state(&self) -> CorpusPlaytestPublicationState {
        self.publication_state
    }
    pub fn source_selection_hash(&self) -> &str {
        &self.source_selection_hash
    }
    pub const fn requested_rooms(&self) -> usize {
        self.requested_rooms
    }
    pub fn entries(&self) -> &[CorpusPlaytestEntry] {
        &self.entries
    }

    pub fn parse(input: &str, allow_provisional: bool) -> Result<Self, CorpusPlaytestError> {
        let manifest = parse_manifest(input, allow_provisional)?;
        if manifest.render() != input {
            return Err(invalid("corpus playtest manifest is not in canonical form"));
        }
        Ok(manifest)
    }

    pub fn from_verified_export(
        publication_state: CorpusPlaytestPublicationState,
        source_selection_hash: String,
        requested_rooms: usize,
        inputs: Vec<CorpusPlaytestEntryInput>,
    ) -> Result<Self, CorpusPlaytestError> {
        if requested_rooms == 0 || requested_rooms > inputs.len() || inputs.len() > MAX_ENTRIES {
            return Err(invalid("invalid requested/exported corpus room count"));
        }
        if !valid_named_hex(&source_selection_hash, "downwards-selection-fnv1a64-") {
            return Err(invalid("invalid source selection hash domain"));
        }
        let entries = inputs
            .into_iter()
            .enumerate()
            .map(|(index, input)| {
                let total_ticks = input.action_spans.iter().try_fold(0usize, |total, span| {
                    total
                        .checked_add(span.ticks)
                        .ok_or_else(|| invalid("action ticks overflow"))
                })?;
                if total_ticks == 0
                    || input
                        .action_spans
                        .iter()
                        .any(|span| span.ticks == 0 || span.action.restart)
                    || input.action_spans.len() > MAX_ACTION_SPANS
                    || total_ticks > MAX_REPLAY_TICKS
                    || input
                        .action_spans
                        .windows(2)
                        .any(|pair| pair[0].action == pair[1].action)
                {
                    return Err(invalid(
                        "exported representative replay is empty, restarts, or has a zero span",
                    ));
                }
                if !valid_named_hex(
                    &input.witness_fingerprint,
                    &format!("downwards-witness-v{WITNESS_FINGERPRINT_VERSION}-"),
                ) {
                    return Err(invalid("invalid witness fingerprint domain"));
                }
                if !valid_named_hex(
                    &input.runtime_replay_checksum,
                    "downwards-corpus-runtime-replay-v1-",
                ) {
                    return Err(invalid("invalid runtime replay checksum domain"));
                }
                Ok(CorpusPlaytestEntry {
                    index,
                    id: input.id,
                    key: input.key,
                    route_selection: input.route_selection,
                    source_door_id: input.source_door_id,
                    target_door_id: input.target_door_id,
                    witness_fingerprint: input.witness_fingerprint,
                    runtime_replay_checksum: input.runtime_replay_checksum,
                    search_stats: input.search_stats,
                    initial_digest: input.initial_digest,
                    sockets: input.sockets,
                    actions: RepresentativeActions {
                        total_ticks,
                        spans: input.action_spans.into_boxed_slice(),
                    },
                })
            })
            .collect::<Result<Vec<_>, CorpusPlaytestError>>()?;
        validate_entries(&entries)?;
        Ok(Self {
            publication_state,
            source_selection_hash,
            requested_rooms,
            entries,
        })
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut body = String::new();
        push_line(&mut body, HEADER);
        push_line(
            &mut body,
            &format!("manifest-version={CORPUS_PLAYTEST_MANIFEST_VERSION}"),
        );
        push_line(
            &mut body,
            &format!("publication-state={}", self.publication_state.slug()),
        );
        push_line(
            &mut body,
            &format!("source-selection-hash={}", self.source_selection_hash),
        );
        push_line(
            &mut body,
            &format!("requested-rooms={}", self.requested_rooms),
        );
        push_line(&mut body, &format!("entry-count={}", self.entries.len()));
        for entry in &self.entries {
            render_entry(&mut body, entry);
        }
        let fingerprint = stable_hash(body.as_bytes());
        body.push_str(&format!("{FINGERPRINT_PREFIX}{fingerprint:016x}\n"));
        body
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusPlaytestError(String);

impl fmt::Display for CorpusPlaytestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Error for CorpusPlaytestError {}

fn invalid(message: impl Into<String>) -> CorpusPlaytestError {
    CorpusPlaytestError(message.into())
}

fn parse_manifest(
    input: &str,
    allow_provisional: bool,
) -> Result<CorpusPlaytestManifest, CorpusPlaytestError> {
    if input.len() > MAX_MANIFEST_BYTES {
        return Err(invalid("corpus playtest manifest exceeds size limit"));
    }
    let (body, fingerprint) = input
        .rsplit_once(FINGERPRINT_PREFIX)
        .ok_or_else(|| invalid("missing corpus playtest manifest fingerprint"))?;
    if !fingerprint.ends_with('\n') || fingerprint[..fingerprint.len() - 1].contains('\n') {
        return Err(invalid(
            "corpus playtest fingerprint must be the final line",
        ));
    }
    let expected = format!("{:016x}\n", stable_hash(body.as_bytes()));
    if fingerprint != expected {
        return Err(invalid("corpus playtest manifest fingerprint mismatch"));
    }
    let mut lines = body.lines();
    exact(&mut lines, HEADER)?;
    exact(
        &mut lines,
        &format!("manifest-version={CORPUS_PLAYTEST_MANIFEST_VERSION}"),
    )?;
    let state = match scalar(&mut lines, "publication-state=")? {
        "final-recomputed" => CorpusPlaytestPublicationState::FinalRecomputed,
        "provisional-operational-cache" if allow_provisional => {
            CorpusPlaytestPublicationState::ProvisionalOperationalCache
        }
        "provisional-operational-cache" => {
            return Err(invalid(
                "provisional corpus manifest requires explicit development opt-in",
            ));
        }
        value => return Err(invalid(format!("unsupported publication state {value:?}"))),
    };
    let source_selection_hash = scalar(&mut lines, "source-selection-hash=")?.to_owned();
    if !valid_named_hex(&source_selection_hash, "downwards-selection-fnv1a64-") {
        return Err(invalid("invalid source selection hash domain"));
    }
    let requested_rooms = number(scalar(&mut lines, "requested-rooms=")?, "requested rooms")?;
    let entry_count: usize = number(scalar(&mut lines, "entry-count=")?, "entry count")?;
    if requested_rooms == 0
        || requested_rooms > entry_count
        || entry_count == 0
        || entry_count > MAX_ENTRIES
    {
        return Err(invalid("invalid requested/encoded corpus room count"));
    }
    let mut entries = Vec::with_capacity(entry_count);
    for index in 0..entry_count {
        entries.push(parse_entry(&mut lines, index)?);
    }
    if lines.next().is_some() {
        return Err(invalid("unexpected trailing corpus manifest lines"));
    }
    validate_entries(&entries)?;
    Ok(CorpusPlaytestManifest {
        publication_state: state,
        source_selection_hash,
        requested_rooms,
        entries,
    })
}

fn render_entry(output: &mut String, entry: &CorpusPlaytestEntry) {
    push_line(output, "entry-begin");
    push_line(output, &format!("id={}", entry.id));
    push_line(output, &format!("generator={}", entry.key.generator_slug()));
    render_key(output, &entry.key);
    push_line(
        output,
        &format!("route-selection={}", entry.route_selection.slug()),
    );
    push_line(output, &format!("source-door={}", entry.source_door_id));
    push_line(output, &format!("target-door={}", entry.target_door_id));
    push_line(
        output,
        &format!("witness-fingerprint={}", entry.witness_fingerprint),
    );
    push_line(
        output,
        &format!("runtime-replay-checksum={}", entry.runtime_replay_checksum),
    );
    push_line(
        output,
        &format!(
            "search-stats={}:{}:{}:{}",
            entry.search_stats.expanded_nodes,
            entry.search_stats.generated_nodes,
            entry.search_stats.simulated_ticks,
            entry.search_stats.deepest_path_ticks,
        ),
    );
    push_line(
        output,
        &format!("initial-digest={:016x}", entry.initial_digest),
    );
    push_line(output, &format!("socket-count={}", entry.sockets.len()));
    for socket in &entry.sockets {
        push_line(
            output,
            &format!(
                "socket={}:{}:{}",
                side_slug(socket.side),
                socket.offset,
                socket.span
            ),
        );
    }
    let actions = entry
        .actions
        .spans()
        .iter()
        .map(|span| {
            let action = span.action;
            format!(
                "{}:{}:{}:{}:{}*{}",
                action.move_x,
                action.move_y,
                u8::from(action.jump),
                u8::from(action.dash),
                u8::from(action.restart),
                span.ticks
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    push_line(output, &format!("actions={actions}"));
    push_line(output, "entry-end");
}

fn render_key(output: &mut String, key: &CorpusPlaytestKey) {
    match key {
        CorpusPlaytestKey::PartitionRoute {
            record_version,
            generator_version,
            source_seed,
            loadout,
            intent,
            profile,
            embedding_attempt,
        } => {
            push_line(output, &format!("key-record-version={record_version}"));
            push_line(output, &format!("generator-version={generator_version}"));
            push_line(output, &format!("source-seed={source_seed}"));
            push_line(output, &format!("construction-loadout={}", loadout.slug()));
            push_line(output, &format!("intent={}", intent.slug()));
            push_line(output, &format!("profile={}", profile.slug()));
            push_line(output, &format!("embedding-attempt={embedding_attempt}"));
        }
        CorpusPlaytestKey::CompositionalRouteCut {
            record_version,
            derivation_version,
            generator_version,
            socket_inventory_version,
            source_seed,
            loadout,
            intent,
            grammar,
            embedding_attempt,
        } => {
            push_line(output, &format!("key-record-version={record_version}"));
            push_line(output, &format!("derivation-version={derivation_version}"));
            push_line(output, &format!("generator-version={generator_version}"));
            push_line(
                output,
                &format!("socket-inventory-version={socket_inventory_version}"),
            );
            push_line(output, &format!("source-seed={source_seed}"));
            push_line(output, &format!("construction-loadout={}", loadout.slug()));
            push_line(output, &format!("intent={}", intent.slug()));
            push_line(output, &format!("grammar={}", grammar.slug()));
            push_line(output, &format!("embedding-attempt={embedding_attempt}"));
        }
        CorpusPlaytestKey::CompositionalAbility {
            record_version,
            generation_version,
            edge_rewrite_version,
            gate_embedding_contract_version,
            base_record_version,
            base_derivation_version,
            base_generator_version,
            base_socket_inventory_version,
            source_seed,
            base_loadout,
            intent,
            base_grammar,
            profile,
            embedding_attempt,
            rewrite_attempt,
        } => {
            push_line(output, &format!("key-record-version={record_version}"));
            push_line(output, &format!("generation-version={generation_version}"));
            push_line(
                output,
                &format!("edge-rewrite-version={edge_rewrite_version}"),
            );
            push_line(
                output,
                &format!("gate-embedding-contract-version={gate_embedding_contract_version}"),
            );
            push_line(
                output,
                &format!("base-record-version={base_record_version}"),
            );
            push_line(
                output,
                &format!("base-derivation-version={base_derivation_version}"),
            );
            push_line(
                output,
                &format!("base-generator-version={base_generator_version}"),
            );
            push_line(
                output,
                &format!("base-socket-inventory-version={base_socket_inventory_version}"),
            );
            push_line(output, &format!("source-seed={source_seed}"));
            push_line(
                output,
                &format!("base-construction-loadout={}", base_loadout.slug()),
            );
            push_line(output, &format!("intent={}", intent.slug()));
            push_line(output, &format!("base-grammar={}", base_grammar.slug()));
            push_line(output, &format!("profile={}", profile.slug()));
            push_line(output, &format!("embedding-attempt={embedding_attempt}"));
            push_line(output, &format!("rewrite-attempt={rewrite_attempt}"));
        }
    }
}

fn push_line(output: &mut String, value: &str) {
    output.push_str(value);
    output.push('\n');
}
fn side_slug(side: BoundarySide) -> &'static str {
    match side {
        BoundarySide::Left => "left",
        BoundarySide::Right => "right",
        BoundarySide::Ceiling => "ceiling",
        BoundarySide::Floor => "floor",
    }
}

fn parse_entry<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    index: usize,
) -> Result<CorpusPlaytestEntry, CorpusPlaytestError> {
    exact(lines, "entry-begin")?;
    let id = scalar(lines, "id=")?.to_owned();
    let generator = scalar(lines, "generator=")?;
    let key = parse_key(lines, generator)?;
    let route_selection = match scalar(lines, "route-selection=")? {
        "authored-source-sink" => CorpusPlaytestRouteSelection::AuthoredSourceSink,
        "deterministic-positive-fallback" => {
            CorpusPlaytestRouteSelection::DeterministicPositiveFallback
        }
        value => return Err(invalid(format!("invalid route selection {value:?}"))),
    };
    let source_door_id = scalar(lines, "source-door=")?.to_owned();
    let target_door_id = scalar(lines, "target-door=")?.to_owned();
    let witness_fingerprint = scalar(lines, "witness-fingerprint=")?.to_owned();
    if !valid_named_hex(
        &witness_fingerprint,
        &format!("downwards-witness-v{WITNESS_FINGERPRINT_VERSION}-"),
    ) {
        return Err(invalid("invalid witness fingerprint"));
    }
    let runtime_replay_checksum = scalar(lines, "runtime-replay-checksum=")?.to_owned();
    if !valid_named_hex(
        &runtime_replay_checksum,
        "downwards-corpus-runtime-replay-v1-",
    ) {
        return Err(invalid("invalid runtime replay checksum"));
    }
    let search_stats = parse_search_stats(scalar(lines, "search-stats=")?)?;
    let initial_digest = u64::from_str_radix(scalar(lines, "initial-digest=")?, 16)
        .map_err(|_| invalid("invalid initial digest"))?;
    let socket_count = number(scalar(lines, "socket-count=")?, "socket count")?;
    if !(2..=4).contains(&socket_count) {
        return Err(invalid("socket count is outside the room contract"));
    }
    let mut sockets = Vec::with_capacity(socket_count);
    for _ in 0..socket_count {
        sockets.push(parse_socket(scalar(lines, "socket=")?)?);
    }
    let actions = parse_action_spans(scalar(lines, "actions=")?)?;
    exact(lines, "entry-end")?;
    Ok(CorpusPlaytestEntry {
        index,
        id,
        key,
        route_selection,
        source_door_id,
        target_door_id,
        witness_fingerprint,
        runtime_replay_checksum,
        search_stats,
        initial_digest,
        sockets,
        actions,
    })
}

fn parse_key<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    generator: &str,
) -> Result<CorpusPlaytestKey, CorpusPlaytestError> {
    let record_version = number(scalar(lines, "key-record-version=")?, "key record version")?;
    match generator {
        "partition-route" => Ok(CorpusPlaytestKey::PartitionRoute {
            record_version,
            generator_version: number(scalar(lines, "generator-version=")?, "generator version")?,
            source_seed: number(scalar(lines, "source-seed=")?, "source seed")?,
            loadout: parse_loadout(scalar(lines, "construction-loadout=")?)?,
            intent: parse_intent(scalar(lines, "intent=")?)?,
            profile: parse_partition_profile(scalar(lines, "profile=")?)?,
            embedding_attempt: number(scalar(lines, "embedding-attempt=")?, "embedding attempt")?,
        }),
        "compositional-route-cut" => Ok(CorpusPlaytestKey::CompositionalRouteCut {
            record_version,
            derivation_version: number(
                scalar(lines, "derivation-version=")?,
                "derivation version",
            )?,
            generator_version: number(scalar(lines, "generator-version=")?, "generator version")?,
            socket_inventory_version: number(
                scalar(lines, "socket-inventory-version=")?,
                "socket inventory version",
            )?,
            source_seed: number(scalar(lines, "source-seed=")?, "source seed")?,
            loadout: parse_loadout(scalar(lines, "construction-loadout=")?)?,
            intent: parse_intent(scalar(lines, "intent=")?)?,
            grammar: parse_route_cut_grammar(scalar(lines, "grammar=")?)?,
            embedding_attempt: number(scalar(lines, "embedding-attempt=")?, "embedding attempt")?,
        }),
        "compositional-ability" => Ok(CorpusPlaytestKey::CompositionalAbility {
            record_version,
            generation_version: number(
                scalar(lines, "generation-version=")?,
                "generation version",
            )?,
            edge_rewrite_version: number(
                scalar(lines, "edge-rewrite-version=")?,
                "edge rewrite version",
            )?,
            gate_embedding_contract_version: number(
                scalar(lines, "gate-embedding-contract-version=")?,
                "gate contract version",
            )?,
            base_record_version: number(
                scalar(lines, "base-record-version=")?,
                "base record version",
            )?,
            base_derivation_version: number(
                scalar(lines, "base-derivation-version=")?,
                "base derivation version",
            )?,
            base_generator_version: number(
                scalar(lines, "base-generator-version=")?,
                "base generator version",
            )?,
            base_socket_inventory_version: number(
                scalar(lines, "base-socket-inventory-version=")?,
                "base socket inventory version",
            )?,
            source_seed: number(scalar(lines, "source-seed=")?, "source seed")?,
            base_loadout: parse_loadout(scalar(lines, "base-construction-loadout=")?)?,
            intent: parse_intent(scalar(lines, "intent=")?)?,
            base_grammar: parse_route_cut_grammar(scalar(lines, "base-grammar=")?)?,
            profile: parse_ability_profile(scalar(lines, "profile=")?)?,
            embedding_attempt: number(scalar(lines, "embedding-attempt=")?, "embedding attempt")?,
            rewrite_attempt: number(scalar(lines, "rewrite-attempt=")?, "rewrite attempt")?,
        }),
        value => Err(invalid(format!("unsupported corpus generator {value:?}"))),
    }
}

fn validate_entries(entries: &[CorpusPlaytestEntry]) -> Result<(), CorpusPlaytestError> {
    if entries.is_empty() {
        return Err(invalid("corpus playtest manifest has no entries"));
    }
    let mut ids = HashSet::new();
    for entry in entries {
        if !ids.insert(entry.id.as_str()) {
            return Err(invalid(format!("duplicate corpus room id {:?}", entry.id)));
        }
        entry.key.validate_versions()?;
        let _ = entry.load_verified()?;
    }
    for entry in entries {
        for socket in &entry.sockets {
            if !entries.iter().any(|candidate| {
                candidate.id != entry.id
                    && candidate
                        .sockets
                        .iter()
                        .any(|other| sockets_mate(*socket, *other))
            }) {
                return Err(invalid(format!(
                    "room {:?} exposes socket without an opposite mate",
                    entry.id
                )));
            }
        }
    }
    Ok(())
}

fn sockets_mate(left: DoorSocket, right: DoorSocket) -> bool {
    left.side.opposite() == right.side && left.offset == right.offset && left.span == right.span
}

fn parse_socket(value: &str) -> Result<DoorSocket, CorpusPlaytestError> {
    let mut fields = value.split(':');
    let side = match fields.next() {
        Some("left") => BoundarySide::Left,
        Some("right") => BoundarySide::Right,
        Some("ceiling") => BoundarySide::Ceiling,
        Some("floor") => BoundarySide::Floor,
        _ => return Err(invalid("invalid socket side")),
    };
    let offset = number(
        fields
            .next()
            .ok_or_else(|| invalid("socket lacks offset"))?,
        "socket offset",
    )?;
    let span = number(
        fields.next().ok_or_else(|| invalid("socket lacks span"))?,
        "socket span",
    )?;
    if fields.next().is_some() {
        return Err(invalid("socket has extra fields"));
    }
    Ok(DoorSocket { side, offset, span })
}

fn parse_action_spans(value: &str) -> Result<RepresentativeActions, CorpusPlaytestError> {
    let mut spans = Vec::new();
    let mut total_ticks = 0usize;
    if value.is_empty() {
        return Err(invalid("representative replay is empty"));
    }
    for encoded in value.split(',') {
        let (action, ticks) = encoded
            .rsplit_once('*')
            .ok_or_else(|| invalid("action span lacks tick count"))?;
        let ticks: usize = number(ticks, "action ticks")?;
        if ticks == 0 {
            return Err(invalid("action span has zero ticks"));
        }
        let parts = action.split(':').collect::<Vec<_>>();
        if parts.len() != 5 {
            return Err(invalid("action span has wrong field count"));
        }
        let move_x = parts[0]
            .parse::<i8>()
            .map_err(|_| invalid("invalid move-x"))?;
        let move_y = parts[1]
            .parse::<i8>()
            .map_err(|_| invalid("invalid move-y"))?;
        if !(-1..=1).contains(&move_x) || !(-1..=1).contains(&move_y) {
            return Err(invalid("movement outside semantic range"));
        }
        let bit = |value: &str| match value {
            "0" => Ok(false),
            "1" => Ok(true),
            _ => Err(invalid("action bit must be 0 or 1")),
        };
        let action = Action {
            move_x,
            move_y,
            jump: bit(parts[2])?,
            dash: bit(parts[3])?,
            restart: bit(parts[4])?,
        };
        if action.restart {
            return Err(invalid("representative route may not restart"));
        }
        if spans
            .last()
            .is_some_and(|span: &ActionSpan| span.action == action)
        {
            return Err(invalid("adjacent identical action spans are not canonical"));
        }
        total_ticks = total_ticks
            .checked_add(ticks)
            .ok_or_else(|| invalid("action ticks overflow"))?;
        spans.push(ActionSpan { action, ticks });
        if spans.len() > MAX_ACTION_SPANS || total_ticks > MAX_REPLAY_TICKS {
            return Err(invalid("representative replay exceeds resource limits"));
        }
    }
    Ok(RepresentativeActions {
        total_ticks,
        spans: spans.into_boxed_slice(),
    })
}

fn parse_search_stats(value: &str) -> Result<SearchStats, CorpusPlaytestError> {
    let values = value
        .split(':')
        .map(|field| number(field, "search statistic"))
        .collect::<Result<Vec<usize>, _>>()?;
    if values.len() != 4 {
        return Err(invalid("search stats require exactly four fields"));
    }
    Ok(SearchStats {
        expanded_nodes: values[0],
        generated_nodes: values[1],
        simulated_ticks: values[2],
        deepest_path_ticks: values[3],
    })
}

fn valid_named_hex(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|hex| {
        hex.len() == 16
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn parse_loadout(value: &str) -> Result<CorpusPlaytestLoadout, CorpusPlaytestError> {
    match value {
        "baseline" => Ok(CorpusPlaytestLoadout::Baseline),
        "wall-jump" => Ok(CorpusPlaytestLoadout::WallJump),
        "dash" => Ok(CorpusPlaytestLoadout::Dash),
        "both" => Ok(CorpusPlaytestLoadout::Both),
        _ => Err(invalid("invalid construction loadout")),
    }
}
fn parse_intent(value: &str) -> Result<CorpusPlaytestIntent, CorpusPlaytestError> {
    match value {
        "gentle" => Ok(CorpusPlaytestIntent::Gentle),
        "standard" => Ok(CorpusPlaytestIntent::Standard),
        "technical" => Ok(CorpusPlaytestIntent::Technical),
        _ => Err(invalid("invalid intent")),
    }
}
fn parse_partition_profile(
    value: &str,
) -> Result<CorpusPlaytestPartitionProfile, CorpusPlaytestError> {
    match value {
        "mixed-bsp" => Ok(CorpusPlaytestPartitionProfile::MixedBsp),
        "columnar" => Ok(CorpusPlaytestPartitionProfile::Columnar),
        "branching" => Ok(CorpusPlaytestPartitionProfile::Branching),
        _ => Err(invalid("invalid partition profile")),
    }
}
fn parse_ability_profile(value: &str) -> Result<CorpusPlaytestAbilityProfile, CorpusPlaytestError> {
    match value {
        "wall-jump" => Ok(CorpusPlaytestAbilityProfile::WallJump),
        "dash" => Ok(CorpusPlaytestAbilityProfile::Dash),
        _ => Err(invalid("invalid ability profile")),
    }
}

fn parse_route_cut_grammar(
    value: &str,
) -> Result<CorpusPlaytestRouteCutGrammar, CorpusPlaytestError> {
    match value {
        "recursive-mission-cuts-v1" => Ok(CorpusPlaytestRouteCutGrammar::RecursiveMissionCutsV1),
        _ => Err(invalid("invalid route-cut grammar")),
    }
}

fn exact<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    expected: &str,
) -> Result<(), CorpusPlaytestError> {
    match lines.next() {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(invalid(format!("expected {expected:?}, found {actual:?}"))),
        None => Err(invalid(format!("expected {expected:?}, found end of file"))),
    }
}
fn scalar<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    prefix: &str,
) -> Result<&'a str, CorpusPlaytestError> {
    lines
        .next()
        .and_then(|line| line.strip_prefix(prefix))
        .ok_or_else(|| invalid(format!("expected {prefix:?}")))
}
fn number<T: std::str::FromStr>(value: &str, name: &str) -> Result<T, CorpusPlaytestError> {
    value
        .parse()
        .map_err(|_| invalid(format!("invalid {name}")))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in b"downwards-corpus-playtest-manifest\0".iter().chain(bytes) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Metadata-independent checksum for the runtime replay carried by the
/// bridge. The upstream witness fingerprint remains provenance for the exact
/// evidence alias; this checksum binds the canonical regenerated room state,
/// ordered route/loadout, actions, state digests, and event digests.
#[must_use]
pub fn corpus_playtest_replay_checksum(
    source_door_id: &str,
    target_door_id: &str,
    loadout: CorpusPlaytestLoadout,
    replay: &Replay,
) -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"downwards-corpus-runtime-replay\0");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    checksum_string(&mut bytes, source_door_id);
    checksum_string(&mut bytes, target_door_id);
    bytes.push(u8::from(loadout.abilities().wall_jump));
    bytes.push(u8::from(loadout.abilities().dash));
    bytes.extend_from_slice(&replay.initial_digest.0.to_le_bytes());
    bytes.extend_from_slice(&(replay.frames.len() as u64).to_le_bytes());
    for frame in &replay.frames {
        bytes.push(frame.action.move_x as u8);
        bytes.push(frame.action.move_y as u8);
        bytes.push(u8::from(frame.action.jump));
        bytes.push(u8::from(frame.action.dash));
        bytes.push(u8::from(frame.action.restart));
        bytes.extend_from_slice(&frame.expected_digest.0.to_le_bytes());
        bytes.extend_from_slice(&frame.expected_event_digest.0.to_le_bytes());
    }
    format!(
        "downwards-corpus-runtime-replay-v1-{:016x}",
        fnv1a64(&bytes)
    )
}

fn checksum_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use downwards_ai::{EventDigest, ReplayFrame};
    use downwards_core::StateDigest;

    fn ability_key() -> CorpusPlaytestKey {
        CorpusPlaytestKey::CompositionalAbility {
            record_version: CORPUS_PLAYTEST_KEY_RECORD_VERSION,
            generation_version: COMPOSITIONAL_ABILITY_GENERATION_VERSION,
            edge_rewrite_version: COMPOSITIONAL_ABILITY_EDGE_REWRITE_VERSION,
            gate_embedding_contract_version: COMPOSITIONAL_ABILITY_GATE_EMBEDDING_CONTRACT_VERSION,
            base_record_version: CORPUS_PLAYTEST_KEY_RECORD_VERSION,
            base_derivation_version: COMPOSITIONAL_ROUTE_CUT_DERIVATION_VERSION,
            base_generator_version: COMPOSITIONAL_ROUTE_CUT_GENERATION_VERSION,
            base_socket_inventory_version: COMPOSITIONAL_ROUTE_CUT_SOCKET_INVENTORY_VERSION,
            source_seed: 73,
            base_loadout: CorpusPlaytestLoadout::Baseline,
            intent: CorpusPlaytestIntent::Technical,
            base_grammar: CorpusPlaytestRouteCutGrammar::RecursiveMissionCutsV1,
            profile: CorpusPlaytestAbilityProfile::Dash,
            embedding_attempt: 4,
            rewrite_attempt: 19,
        }
    }

    #[test]
    fn exact_key_wire_round_trip_preserves_base_grammar_and_loadout() {
        let key = ability_key();
        let mut rendered = String::new();
        render_key(&mut rendered, &key);
        let mut lines = rendered.lines();
        let parsed = parse_key(&mut lines, key.generator_slug()).unwrap();
        assert_eq!(parsed, key);
        assert!(lines.next().is_none());
        assert_eq!(parsed.construction_loadout(), CorpusPlaytestLoadout::Dash);

        let mut wrong_version = parsed;
        let CorpusPlaytestKey::CompositionalAbility { record_version, .. } = &mut wrong_version
        else {
            unreachable!()
        };
        *record_version += 1;
        assert!(wrong_version.validate_versions().is_err());
    }

    #[test]
    fn malformed_replay_schema_and_hash_domains_are_rejected() {
        assert!(parse_action_spans("0:0:0:0:0*1,0:0:0:0:0*2").is_err());
        assert!(parse_action_spans("0:0:0:0:1*1").is_err());
        assert!(parse_action_spans("2:0:0:0:0*1").is_err());
        assert!(!valid_named_hex(
            "downwards-corpus-runtime-replay-v1-ABCDEF0123456789",
            "downwards-corpus-runtime-replay-v1-"
        ));
        assert!(!valid_named_hex(
            "downwards-corpus-runtime-replay-v2-abcdef0123456789",
            "downwards-corpus-runtime-replay-v1-"
        ));
    }

    #[test]
    fn runtime_replay_checksum_binds_original_frame_evidence_without_generator_metadata() {
        let replay = Replay {
            initial_digest: StateDigest(11),
            frames: vec![ReplayFrame {
                action: Action {
                    move_x: 1,
                    jump: true,
                    ..Action::default()
                },
                expected_digest: StateDigest(12),
                expected_event_digest: EventDigest(13),
            }],
        };
        let checksum = corpus_playtest_replay_checksum(
            "port-0",
            "port-1",
            CorpusPlaytestLoadout::Dash,
            &replay,
        );
        assert!(valid_named_hex(
            &checksum,
            "downwards-corpus-runtime-replay-v1-"
        ));
        assert_ne!(
            checksum,
            corpus_playtest_replay_checksum(
                "port-0",
                "port-2",
                CorpusPlaytestLoadout::Dash,
                &replay,
            )
        );
        assert_ne!(
            checksum,
            corpus_playtest_replay_checksum(
                "port-0",
                "port-1",
                CorpusPlaytestLoadout::Baseline,
                &replay,
            )
        );
    }
}
