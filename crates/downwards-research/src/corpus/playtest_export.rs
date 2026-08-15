//! Trusted export of a small, native-keyed corpus slice for the game client.

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    fs::{OpenOptions, read},
    io::Write,
    path::Path,
};

use downwards_catalogue::{
    ActionSpan, CorpusPlaytestAbilityProfile, CorpusPlaytestEntryInput, CorpusPlaytestIntent,
    CorpusPlaytestKey, CorpusPlaytestLoadout, CorpusPlaytestManifest,
    CorpusPlaytestPartitionProfile, CorpusPlaytestPublicationState, CorpusPlaytestRouteCutGrammar,
    CorpusPlaytestRouteSelection, corpus_playtest_replay_checksum,
};
use downwards_core::{Action, DoorSocket};
use downwards_validation::BoundedTargetEvidence;

use super::offline_selection_cache::stable_byte_hash as offline_cache_byte_hash;
use super::{
    ChallengeIntentRecord, CompositionalAbilityGateProfileRecord, CorpusCandidateKeyRecord,
    EvaluationLoadout, OfflineSelectionPublicationStateV1, PartitionRouteProfileRecord, RoomId,
    load_offline_selection_artifact_v1, load_source_bound_offline_cache_v1,
    resolve_corpus_metric_candidate_v2, verify_final_offline_selection_v1,
};

pub const DEFAULT_CORPUS_PLAYTEST_REQUESTED_ROOMS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvisionalPlaytestPolicy {
    Reject,
    AllowExplicitDevelopmentFallback,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusPlaytestExportSummary {
    pub requested_rooms: usize,
    pub exported_rooms: usize,
    pub socket_mates_added: usize,
    pub authored_routes: usize,
    pub fallback_routes: usize,
}

pub fn export_corpus_playtest_manifest(
    cache_root: &Path,
    shard_root: &Path,
    selection_directory: &Path,
    output_file: &Path,
    requested_rooms: usize,
    provisional_policy: ProvisionalPlaytestPolicy,
) -> Result<CorpusPlaytestExportSummary, CorpusPlaytestExportError> {
    if requested_rooms == 0 {
        return Err(invalid(
            "playtest export requires at least one requested room",
        ));
    }
    let selection = match provisional_policy {
        ProvisionalPlaytestPolicy::Reject => {
            verify_final_offline_selection_v1(cache_root, shard_root, selection_directory).map_err(
                |error| invalid(format!("final selection did not fully reverify: {error}")),
            )?
        }
        ProvisionalPlaytestPolicy::AllowExplicitDevelopmentFallback => {
            load_offline_selection_artifact_v1(selection_directory).map_err(|error| {
                invalid(format!("could not load development selection: {error}"))
            })?
        }
    };
    let publication_state =
        playtest_publication_state(selection.publication_state, provisional_policy)?;
    let selection_bytes = read(selection_directory.join("selection.json"))
        .map_err(|error| invalid(format!("could not read source selection bytes: {error}")))?;
    if load_offline_selection_artifact_v1(selection_directory).map_err(|error| {
        invalid(format!(
            "could not recheck selection after byte read: {error}"
        ))
    })? != selection
    {
        return Err(invalid("selection changed while preparing playtest export"));
    }
    let selected = &selection.outcome.selected_room_ids;
    if requested_rooms > selected.len() {
        return Err(invalid(format!(
            "requested {requested_rooms} rooms but selection contains only {}",
            selected.len()
        )));
    }
    let descriptors = selection
        .outcome
        .descriptors
        .iter()
        .map(|descriptor| {
            descriptor
                .to_descriptor()
                .map(|decoded| (decoded.room_id, decoded.sockets))
                .map_err(|error| invalid(format!("invalid selected descriptor: {error}")))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let chosen_ids = socket_closed_qd_prefix(selected, &descriptors, requested_rooms)?;

    // This is the trust bridge: cache hashes/checkpoints are checked, every
    // source shard is independently verified and replay-rehydrated, and cache
    // room identities/keys/sockets are rebound to that exact source pool.
    let source_bound =
        load_source_bound_offline_cache_v1(cache_root, shard_root).map_err(|error| {
            invalid(format!(
                "could not bind export to exact source corpus: {error}"
            ))
        })?;
    let completion_hash = offline_cache_byte_hash(
        &read(cache_root.join("cache-checkpoint.json"))
            .map_err(|error| invalid(format!("could not read cache completion: {error}")))?,
    );
    if selection.source_cache_run_id != source_bound.cache.manifest.run_id
        || selection.source_cache_completion_hash != completion_hash
        || selection.analysis_config != source_bound.cache.manifest.analysis_config
        || selection.policies != source_bound.cache.manifest.policies
    {
        return Err(invalid(
            "selection does not match the exact source-bound cache identity",
        ));
    }
    let evaluated = source_bound
        .corpus
        .rooms
        .iter()
        .map(|room| (&room.generated.id, room))
        .collect::<BTreeMap<_, _>>();
    let cache_rows = source_bound
        .cache
        .rooms
        .iter()
        .map(|room| (&room.room_id, room))
        .collect::<BTreeMap<_, _>>();

    let mut authored_routes = 0usize;
    let mut fallback_routes = 0usize;
    let mut inputs = Vec::with_capacity(chosen_ids.len());
    for room_id in &chosen_ids {
        let room = evaluated.get(room_id).ok_or_else(|| {
            invalid(format!(
                "selected room {:?} is absent from exact sources",
                room_id.0
            ))
        })?;
        let cache_row = cache_rows.get(room_id).ok_or_else(|| {
            invalid(format!(
                "selected room {:?} is absent from verified cache",
                room_id.0
            ))
        })?;
        let candidate = resolve_corpus_metric_candidate_v2(room).map_err(|error| {
            invalid(format!(
                "room {:?} has no exact canonical candidate: {error}",
                room_id.0
            ))
        })?;
        if candidate.exact_key() != cache_row.canonical_key {
            return Err(invalid(format!(
                "room {:?} canonical key changed during export",
                room_id.0
            )));
        }
        let evaluation_loadout = candidate.exact_key().construction_loadout();
        let matrix = room
            .matrices
            .iter()
            .find(|matrix| matrix.loadout == evaluation_loadout)
            .ok_or_else(|| {
                invalid(format!(
                    "room {:?} lacks its construction-loadout matrix",
                    room_id.0
                ))
            })?;
        let authored_pair = authored_pair(&candidate.exact_key());
        let (route, route_selection) = matrix
            .evidence
            .door_routes()
            .iter()
            .find(|route| {
                authored_pair.as_ref().is_some_and(|(source, target)| {
                    route.source_door_id == *source && route.target_door_id == *target
                }) && route.evidence.positive().is_some()
            })
            .map(|route| (route, CorpusPlaytestRouteSelection::AuthoredSourceSink))
            .or_else(|| {
                matrix
                    .evidence
                    .door_routes()
                    .iter()
                    .find(|route| route.evidence.positive().is_some())
                    .map(|route| {
                        (
                            route,
                            CorpusPlaytestRouteSelection::DeterministicPositiveFallback,
                        )
                    })
            })
            .ok_or_else(|| {
                invalid(format!(
                    "room {:?} has no positive construction-loadout door route",
                    room_id.0
                ))
            })?;
        match route_selection {
            CorpusPlaytestRouteSelection::AuthoredSourceSink => authored_routes += 1,
            CorpusPlaytestRouteSelection::DeterministicPositiveFallback => fallback_routes += 1,
        }
        let BoundedTargetEvidence::Positive(positive) = &route.evidence else {
            unreachable!()
        };
        let replay = &positive.solution().replay;
        let sockets = candidate
            .boundary_ports()
            .iter()
            .map(|port| port.door.socket())
            .collect::<Vec<_>>();
        if sockets
            != cache_row
                .descriptor
                .to_descriptor()
                .map_err(|error| invalid(format!("invalid cached descriptor: {error}")))?
                .sockets
        {
            return Err(invalid(format!(
                "room {:?} socket descriptor changed during export",
                room_id.0
            )));
        }
        inputs.push(CorpusPlaytestEntryInput {
            id: room_id.0.clone(),
            key: catalogue_key(&cache_row.canonical_key),
            route_selection,
            source_door_id: route.source_door_id.clone(),
            target_door_id: route.target_door_id.clone(),
            witness_fingerprint: positive.witness_fingerprint().to_string(),
            runtime_replay_checksum: corpus_playtest_replay_checksum(
                &route.source_door_id,
                &route.target_door_id,
                loadout(evaluation_loadout),
                replay,
            ),
            initial_digest: replay.initial_digest.0,
            search_stats: positive.solution().stats,
            sockets,
            action_spans: rle_actions(replay.actions()),
        });
    }
    let source_selection_hash = format!(
        "downwards-selection-fnv1a64-{:016x}",
        stable_hash(&selection_bytes)
    );
    let manifest = CorpusPlaytestManifest::from_verified_export(
        publication_state,
        source_selection_hash,
        requested_rooms,
        inputs,
    )
    .map_err(|error| invalid(format!("could not construct playtest manifest: {error}")))?;
    let rendered = manifest.render();
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output_file)
        .map_err(|error| {
            invalid(format!(
                "could not create {}: {error}",
                output_file.display()
            ))
        })?;
    output.write_all(rendered.as_bytes()).map_err(|error| {
        invalid(format!(
            "could not write {}: {error}",
            output_file.display()
        ))
    })?;
    output
        .sync_all()
        .map_err(|error| invalid(format!("could not sync {}: {error}", output_file.display())))?;
    let reparsed = CorpusPlaytestManifest::parse(
        &String::from_utf8(read(output_file).map_err(|error| invalid(error.to_string()))?)
            .map_err(|error| invalid(error.to_string()))?,
        provisional_policy == ProvisionalPlaytestPolicy::AllowExplicitDevelopmentFallback,
    )
    .map_err(|error| invalid(format!("written playtest manifest did not verify: {error}")))?;
    if reparsed != manifest {
        return Err(invalid(
            "written playtest manifest changed on strict readback",
        ));
    }
    Ok(CorpusPlaytestExportSummary {
        requested_rooms,
        exported_rooms: chosen_ids.len(),
        socket_mates_added: chosen_ids.len() - requested_rooms,
        authored_routes,
        fallback_routes,
    })
}

fn playtest_publication_state(
    state: OfflineSelectionPublicationStateV1,
    provisional_policy: ProvisionalPlaytestPolicy,
) -> Result<CorpusPlaytestPublicationState, CorpusPlaytestExportError> {
    match state {
        OfflineSelectionPublicationStateV1::FinalRecomputed
            if provisional_policy == ProvisionalPlaytestPolicy::Reject =>
        {
            Ok(CorpusPlaytestPublicationState::FinalRecomputed)
        }
        OfflineSelectionPublicationStateV1::FinalRecomputed => Err(invalid(
            "the explicit development export accepts provisional selections only; use the trusted final export command for final-recomputed input",
        )),
        OfflineSelectionPublicationStateV1::ProvisionalOperationalCache
            if provisional_policy
                == ProvisionalPlaytestPolicy::AllowExplicitDevelopmentFallback =>
        {
            Ok(CorpusPlaytestPublicationState::ProvisionalOperationalCache)
        }
        OfflineSelectionPublicationStateV1::ProvisionalOperationalCache => Err(invalid(
            "playtest export requires final-recomputed selection; provisional use needs the explicit development command",
        )),
    }
}

fn socket_closed_qd_prefix(
    selected: &[RoomId],
    descriptors: &BTreeMap<RoomId, Vec<DoorSocket>>,
    requested: usize,
) -> Result<Vec<RoomId>, CorpusPlaytestExportError> {
    let mut chosen = selected
        .iter()
        .take(requested)
        .cloned()
        .collect::<BTreeSet<_>>();
    loop {
        let missing = selected
            .iter()
            .filter(|id| chosen.contains(*id))
            .flat_map(|provider_id| {
                descriptors
                    .get(provider_id)
                    .into_iter()
                    .flat_map(|sockets| {
                        sockets
                            .iter()
                            .copied()
                            .map(|socket| ((*provider_id).clone(), socket))
                    })
            })
            .find(|(provider_id, socket)| {
                !selected
                    .iter()
                    .filter(|id| chosen.contains(*id) && *id != provider_id)
                    .any(|id| {
                        descriptors.get(id).is_some_and(|sockets| {
                            sockets
                                .iter()
                                .any(|candidate| sockets_mate(*socket, *candidate))
                        })
                    })
            });
        let Some((provider_id, missing)) = missing else {
            break;
        };
        let mate = selected
            .iter()
            .find(|id| {
                !chosen.contains(*id)
                    && **id != provider_id
                    && descriptors.get(*id).is_some_and(|sockets| {
                        sockets
                            .iter()
                            .any(|candidate| sockets_mate(missing, *candidate))
                    })
            })
            .ok_or_else(|| {
                invalid(format!(
                    "selected set has no different-room mate for socket {:?} from {:?}",
                    missing, provider_id.0
                ))
            })?;
        chosen.insert(mate.clone());
    }
    Ok(selected
        .iter()
        .filter(|id| chosen.contains(*id))
        .cloned()
        .collect())
}

fn sockets_mate(left: DoorSocket, right: DoorSocket) -> bool {
    left.side.opposite() == right.side && left.offset == right.offset && left.span == right.span
}

fn authored_pair(key: &CorpusCandidateKeyRecord) -> Option<(String, String)> {
    match key {
        CorpusCandidateKeyRecord::PartitionRoute(_) => {
            Some(("port-west".into(), "port-east".into()))
        }
        CorpusCandidateKeyRecord::CompositionalRouteCut(_)
        | CorpusCandidateKeyRecord::CompositionalAbility(_) => {
            Some(("port-0".into(), "port-1".into()))
        }
    }
}

fn catalogue_key(key: &CorpusCandidateKeyRecord) -> CorpusPlaytestKey {
    match key {
        CorpusCandidateKeyRecord::PartitionRoute(record) => CorpusPlaytestKey::PartitionRoute {
            record_version: record.record_version,
            generator_version: record.generator_version,
            source_seed: record.source_seed,
            loadout: loadout(record.construction_loadout),
            intent: intent(record.intent),
            profile: match record.profile {
                PartitionRouteProfileRecord::MixedBsp => CorpusPlaytestPartitionProfile::MixedBsp,
                PartitionRouteProfileRecord::Columnar => CorpusPlaytestPartitionProfile::Columnar,
                PartitionRouteProfileRecord::Branching => CorpusPlaytestPartitionProfile::Branching,
            },
            embedding_attempt: record.embedding_attempt,
        },
        CorpusCandidateKeyRecord::CompositionalRouteCut(record) => {
            CorpusPlaytestKey::CompositionalRouteCut {
                record_version: record.record_version,
                derivation_version: record.derivation_version,
                generator_version: record.generator_version,
                socket_inventory_version: record.socket_inventory_version,
                source_seed: record.source_seed,
                loadout: loadout(record.construction_loadout),
                intent: intent(record.intent),
                grammar: route_cut_grammar(record.grammar),
                embedding_attempt: record.embedding_attempt,
            }
        }
        CorpusCandidateKeyRecord::CompositionalAbility(record) => {
            CorpusPlaytestKey::CompositionalAbility {
                record_version: record.record_version,
                generation_version: record.generation_version,
                edge_rewrite_version: record.edge_rewrite_version,
                gate_embedding_contract_version: record.gate_embedding_contract_version,
                base_record_version: record.base_key.record_version,
                base_derivation_version: record.base_key.derivation_version,
                base_generator_version: record.base_key.generator_version,
                base_socket_inventory_version: record.base_key.socket_inventory_version,
                source_seed: record.base_key.source_seed,
                base_loadout: loadout(record.base_key.construction_loadout),
                intent: intent(record.base_key.intent),
                base_grammar: route_cut_grammar(record.base_key.grammar),
                profile: match record.profile {
                    CompositionalAbilityGateProfileRecord::WallJump => {
                        CorpusPlaytestAbilityProfile::WallJump
                    }
                    CompositionalAbilityGateProfileRecord::Dash => {
                        CorpusPlaytestAbilityProfile::Dash
                    }
                },
                embedding_attempt: record.base_key.embedding_attempt,
                rewrite_attempt: record.rewrite_attempt,
            }
        }
    }
}

fn loadout(value: EvaluationLoadout) -> CorpusPlaytestLoadout {
    match value {
        EvaluationLoadout::Baseline => CorpusPlaytestLoadout::Baseline,
        EvaluationLoadout::WallJump => CorpusPlaytestLoadout::WallJump,
        EvaluationLoadout::Dash => CorpusPlaytestLoadout::Dash,
        EvaluationLoadout::Both => CorpusPlaytestLoadout::Both,
    }
}
fn intent(value: ChallengeIntentRecord) -> CorpusPlaytestIntent {
    match value {
        ChallengeIntentRecord::Gentle => CorpusPlaytestIntent::Gentle,
        ChallengeIntentRecord::Standard => CorpusPlaytestIntent::Standard,
        ChallengeIntentRecord::Technical => CorpusPlaytestIntent::Technical,
    }
}

fn route_cut_grammar(
    value: super::CompositionalRouteCutGrammarRecord,
) -> CorpusPlaytestRouteCutGrammar {
    match value {
        super::CompositionalRouteCutGrammarRecord::RecursiveMissionCutsV1 => {
            CorpusPlaytestRouteCutGrammar::RecursiveMissionCutsV1
        }
    }
}

fn rle_actions(actions: impl Iterator<Item = Action>) -> Vec<ActionSpan> {
    let mut spans: Vec<ActionSpan> = Vec::new();
    for action in actions {
        if let Some(last) = spans.last_mut().filter(|span| span.action == action) {
            last.ticks += 1;
        } else {
            spans.push(ActionSpan { action, ticks: 1 });
        }
    }
    spans
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in b"downwards-playtest-source-selection\0".iter().chain(bytes) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorpusPlaytestExportError(String);
impl fmt::Display for CorpusPlaytestExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl Error for CorpusPlaytestExportError {}
fn invalid(message: impl Into<String>) -> CorpusPlaytestExportError {
    CorpusPlaytestExportError(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use downwards_core::BoundarySide;

    fn room_id(value: &str) -> RoomId {
        RoomId(value.to_owned())
    }

    fn socket(side: BoundarySide, offset: i32) -> DoorSocket {
        DoorSocket {
            side,
            offset,
            span: 2,
        }
    }

    #[test]
    fn socket_closure_cascades_in_qd_order_and_uses_other_rooms() {
        let a = room_id("a");
        let b = room_id("b");
        let c = room_id("c");
        let selected = vec![a.clone(), b.clone(), c.clone()];
        let descriptors = BTreeMap::from([
            (a.clone(), vec![socket(BoundarySide::Right, 0)]),
            (
                b.clone(),
                vec![
                    socket(BoundarySide::Left, 0),
                    socket(BoundarySide::Right, 4),
                ],
            ),
            (c.clone(), vec![socket(BoundarySide::Left, 4)]),
        ]);

        assert_eq!(
            socket_closed_qd_prefix(&selected, &descriptors, 1).unwrap(),
            selected
        );

        let same_room_only = BTreeMap::from([(
            a.clone(),
            vec![
                socket(BoundarySide::Left, 0),
                socket(BoundarySide::Right, 0),
            ],
        )]);
        assert!(socket_closed_qd_prefix(&[a], &same_room_only, 1).is_err());
    }

    #[test]
    fn development_export_cannot_launder_a_final_selection() {
        assert!(
            playtest_publication_state(
                OfflineSelectionPublicationStateV1::FinalRecomputed,
                ProvisionalPlaytestPolicy::AllowExplicitDevelopmentFallback,
            )
            .is_err()
        );
        assert_eq!(
            playtest_publication_state(
                OfflineSelectionPublicationStateV1::ProvisionalOperationalCache,
                ProvisionalPlaytestPolicy::AllowExplicitDevelopmentFallback,
            )
            .unwrap(),
            CorpusPlaytestPublicationState::ProvisionalOperationalCache
        );
    }
}
