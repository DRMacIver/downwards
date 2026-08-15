use std::{error::Error, fmt};

use downwards_validation::{
    BoundedTargetEvidence, DoorTargetEvidenceBatch, DoorTargetEvidenceError, ValidationConfig,
    evaluate_generated_door_targets_for_loadout,
};

use super::{EvaluationLoadout, GeneratedCorpusBatch, GeneratedCorpusRoom, RoomId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteMatrixSummary {
    pub door_rows: usize,
    pub positive_door_rows: usize,
    pub inconclusive_door_rows: usize,
    pub pickup_rows: usize,
    pub positive_pickup_rows: usize,
    pub inconclusive_pickup_rows: usize,
}

impl RouteMatrixSummary {
    fn from_batch(batch: &DoorTargetEvidenceBatch) -> Self {
        let positive_door_rows = batch
            .door_routes()
            .iter()
            .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
            .count();
        let positive_pickup_rows = batch
            .pickup_routes()
            .iter()
            .filter(|row| matches!(row.evidence, BoundedTargetEvidence::Positive(_)))
            .count();
        Self {
            door_rows: batch.door_routes().len(),
            positive_door_rows,
            inconclusive_door_rows: batch.door_routes().len() - positive_door_rows,
            pickup_rows: batch.pickup_routes().len(),
            positive_pickup_rows,
            inconclusive_pickup_rows: batch.pickup_routes().len() - positive_pickup_rows,
        }
    }

    #[must_use]
    pub const fn all_positive(self) -> bool {
        self.inconclusive_door_rows == 0 && self.inconclusive_pickup_rows == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadoutRouteMatrix {
    pub loadout: EvaluationLoadout,
    pub evidence: DoorTargetEvidenceBatch,
    pub summary: RouteMatrixSummary,
}

#[derive(Clone, Debug)]
pub struct EvaluatedCorpusRoom {
    pub generated: GeneratedCorpusRoom,
    pub matrices: Vec<LoadoutRouteMatrix>,
}

impl EvaluatedCorpusRoom {
    /// Historical staged-v6 pilot gate using that artifact's canonical first
    /// variant. Final multi-generator corpus builds must evaluate constructive
    /// derivations explicitly instead of treating vector order as authority.
    #[must_use]
    pub fn construction_loadout_gate_passes(&self) -> bool {
        let intended = self.generated.variants[0].key.source.profile.abilities;
        self.matrices
            .iter()
            .any(|matrix| matrix.loadout.abilities() == intended && matrix.summary.all_positive())
    }

    #[must_use]
    pub fn complete_kit_gate_passes(&self) -> bool {
        self.matrices.iter().any(|matrix| {
            matrix.loadout == EvaluationLoadout::Both && matrix.summary.all_positive()
        })
    }
}

#[derive(Clone, Debug)]
pub struct EvaluatedCorpusBatch {
    pub generated: GeneratedCorpusBatch,
    pub rooms: Vec<EvaluatedCorpusRoom>,
}

pub fn evaluate_route_matrices(
    generated: GeneratedCorpusBatch,
) -> Result<EvaluatedCorpusBatch, CorpusEvaluationError> {
    evaluate_route_matrices_with(generated, |loadout| {
        ValidationConfig::for_loadout(loadout.abilities())
    })
}

pub fn evaluate_route_matrices_with(
    mut generated: GeneratedCorpusBatch,
    mut config_for_loadout: impl FnMut(EvaluationLoadout) -> ValidationConfig,
) -> Result<EvaluatedCorpusBatch, CorpusEvaluationError> {
    let source_rooms = std::mem::take(&mut generated.rooms);
    let mut rooms = Vec::with_capacity(source_rooms.len());
    for room in source_rooms {
        let mut matrices = Vec::with_capacity(EvaluationLoadout::ALL.len());
        for loadout in EvaluationLoadout::ALL {
            let candidate = &room.variants[0];
            let config = config_for_loadout(loadout);
            let evidence = evaluate_generated_door_targets_for_loadout(
                &candidate.generated,
                loadout.abilities(),
                &config,
            )
            .map_err(|source| CorpusEvaluationError::Evidence {
                room_id: room.id.clone(),
                loadout,
                source: Box::new(source),
            })?;
            let door_count = candidate.generated.room.doors().len();
            let pickup_count = candidate.generated.room.pickups().len();
            let expected_door_rows = door_count.saturating_mul(door_count.saturating_sub(1));
            let expected_pickup_rows = door_count.saturating_mul(pickup_count);
            if evidence.door_routes().len() != expected_door_rows
                || evidence.pickup_routes().len() != expected_pickup_rows
            {
                return Err(CorpusEvaluationError::MatrixCardinality {
                    room_id: room.id.clone(),
                    loadout,
                    expected_door_rows,
                    actual_door_rows: evidence.door_routes().len(),
                    expected_pickup_rows,
                    actual_pickup_rows: evidence.pickup_routes().len(),
                });
            }
            let summary = RouteMatrixSummary::from_batch(&evidence);
            matrices.push(LoadoutRouteMatrix {
                loadout,
                evidence,
                summary,
            });
        }
        rooms.push(EvaluatedCorpusRoom {
            generated: room,
            matrices,
        });
    }
    Ok(EvaluatedCorpusBatch { generated, rooms })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CorpusEvaluationError {
    Evidence {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        source: Box<DoorTargetEvidenceError>,
    },
    MatrixCardinality {
        room_id: RoomId,
        loadout: EvaluationLoadout,
        expected_door_rows: usize,
        actual_door_rows: usize,
        expected_pickup_rows: usize,
        actual_pickup_rows: usize,
    },
}

impl fmt::Display for CorpusEvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Evidence {
                room_id,
                loadout,
                source,
            } => write!(
                formatter,
                "route evidence failed for {} under {}: {source}",
                room_id.0,
                loadout.slug()
            ),
            Self::MatrixCardinality {
                room_id,
                loadout,
                expected_door_rows,
                actual_door_rows,
                expected_pickup_rows,
                actual_pickup_rows,
            } => write!(
                formatter,
                "route matrix for {} under {} has {actual_door_rows}/{expected_door_rows} door rows and {actual_pickup_rows}/{expected_pickup_rows} pickup rows",
                room_id.0,
                loadout.slug()
            ),
        }
    }
}

impl Error for CorpusEvaluationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Evidence { source, .. } => Some(source.as_ref()),
            Self::MatrixCardinality { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::{CorpusBuildConfigV1, generate_seed_block};

    #[test]
    fn bounded_evaluation_fills_every_matrix_cell_without_false_unreachable_claims() {
        let generated = generate_seed_block(CorpusBuildConfigV1::terrain_only_pilot(0, 1)).unwrap();
        let first_room = generated.rooms[0].clone();
        let mut one_room = generated;
        one_room.rooms = vec![first_room];
        let evaluated = evaluate_route_matrices_with(one_room, |loadout| {
            let mut config = ValidationConfig::for_loadout(loadout.abilities());
            config.solver.max_expanded_nodes = 1;
            config.solver.max_simulated_ticks = 2_000;
            config
        })
        .unwrap();

        assert_eq!(evaluated.rooms.len(), 1);
        let room = &evaluated.rooms[0];
        assert_eq!(room.matrices.len(), 4);
        for matrix in &room.matrices {
            let door_count = room.generated.variants[0].generated.room.doors().len();
            let pickup_count = room.generated.variants[0].generated.room.pickups().len();
            assert_eq!(matrix.summary.door_rows, door_count * (door_count - 1));
            assert_eq!(matrix.summary.pickup_rows, door_count * pickup_count);
            assert_eq!(
                matrix.summary.positive_door_rows + matrix.summary.inconclusive_door_rows,
                matrix.summary.door_rows
            );
            assert_eq!(
                matrix.summary.positive_pickup_rows + matrix.summary.inconclusive_pickup_rows,
                matrix.summary.pickup_rows
            );
        }
    }
}
