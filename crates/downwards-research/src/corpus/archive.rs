//! Deterministic quality-diversity archiving and representative selection.
//!
//! The archive deliberately has no room-specific knowledge. Callers provide
//! stable candidate identifiers, integer-quantized cell projections, a
//! directed Pareto-quality vector, and integer diversity coordinates. Each
//! projection is archived independently: two coordinates from different
//! projections are never concatenated into one sparse Cartesian mega-cell.
//!
//! Quality coordinates are used only for Pareto dominance and cell crowding.
//! Diversity coordinates are used only by farthest-point selection. There is
//! no weighted scalar "fun" score in either stage.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    ops::RangeInclusive,
};

/// Whether a quality coordinate is better when its integer value rises or
/// falls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectiveDirection {
    Maximize,
    Minimize,
}

/// Configuration shared by every cell in one archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveConfig {
    /// The comparison direction of each coordinate in a candidate's quality
    /// vector. Coordinates are never weighted or summed.
    pub quality_directions: Vec<ObjectiveDirection>,
    /// Maximum number of mutually non-dominated elites retained in one cell.
    pub elites_per_cell: usize,
}

impl ArchiveConfig {
    #[must_use]
    pub fn new(quality_directions: Vec<ObjectiveDirection>, elites_per_cell: usize) -> Self {
        Self {
            quality_directions,
            elites_per_cell,
        }
    }
}

/// One cell assignment in one independently evaluated descriptor projection.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectedCell<ProjectionId> {
    pub projection: ProjectionId,
    pub coordinates: Vec<i64>,
}

impl<ProjectionId> ProjectedCell<ProjectionId> {
    #[must_use]
    pub fn new(projection: ProjectionId, coordinates: Vec<i64>) -> Self {
        Self {
            projection,
            coordinates,
        }
    }
}

/// Precomputed input to [`QualityDiversityArchive::build`].
///
/// `quality` participates only in directed Pareto comparisons. The L1 metric
/// over `diversity_coordinates` participates only in representative
/// selection. Callers should quantize diversity coordinate groups before
/// constructing this value when raw groups have incomparable scales.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveCandidate<CandidateId, ProjectionId> {
    pub id: CandidateId,
    pub projections: Vec<ProjectedCell<ProjectionId>>,
    pub quality: Vec<i64>,
    pub diversity_coordinates: Vec<i64>,
}

impl<CandidateId, ProjectionId> ArchiveCandidate<CandidateId, ProjectionId> {
    #[must_use]
    pub fn new(
        id: CandidateId,
        projections: Vec<ProjectedCell<ProjectionId>>,
        quality: Vec<i64>,
        diversity_coordinates: Vec<i64>,
    ) -> Self {
        Self {
            id,
            projections,
            quality,
            diversity_coordinates,
        }
    }
}

/// Stable key for one cell in one projection.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArchiveCellKey<ProjectionId> {
    pub projection: ProjectionId,
    pub coordinates: Vec<i64>,
}

impl<ProjectionId> From<ProjectedCell<ProjectionId>> for ArchiveCellKey<ProjectionId> {
    fn from(value: ProjectedCell<ProjectionId>) -> Self {
        Self {
            projection: value.projection,
            coordinates: value.coordinates,
        }
    }
}

/// A bounded, deterministic Pareto set for one projected cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveCell<CandidateId> {
    elites: Vec<CandidateId>,
}

impl<CandidateId> ArchiveCell<CandidateId> {
    /// Elites ordered by stable candidate identifier.
    #[must_use]
    pub fn elites(&self) -> &[CandidateId] {
        &self.elites
    }
}

/// Aggregate counts from archive construction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArchiveBuildSummary {
    pub submitted_candidates: usize,
    pub retained_candidates: usize,
    pub cell_count: usize,
    pub elite_placements: usize,
}

/// Invalid precomputed archive input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveError {
    EmptyQualityVector,
    ZeroCellCapacity,
    DuplicateCandidateId,
    CandidateWithoutProjection,
    DuplicateProjectionForCandidate,
    QualityDimension { expected: usize, actual: usize },
    EmptyDiversityVector,
    DiversityDimension { expected: usize, actual: usize },
    ProjectionDimension { expected: usize, actual: usize },
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyQualityVector => formatter.write_str("archive quality vector is empty"),
            Self::ZeroCellCapacity => {
                formatter.write_str("archive cell capacity must be greater than zero")
            }
            Self::DuplicateCandidateId => {
                formatter.write_str("archive candidate identifiers must be unique")
            }
            Self::CandidateWithoutProjection => {
                formatter.write_str("archive candidate has no descriptor projection")
            }
            Self::DuplicateProjectionForCandidate => formatter
                .write_str("archive candidate has more than one cell in the same projection"),
            Self::QualityDimension { expected, actual } => write!(
                formatter,
                "archive quality dimension mismatch: expected {expected}, got {actual}",
            ),
            Self::EmptyDiversityVector => formatter.write_str("archive diversity vector is empty"),
            Self::DiversityDimension { expected, actual } => write!(
                formatter,
                "archive diversity dimension mismatch: expected {expected}, got {actual}",
            ),
            Self::ProjectionDimension { expected, actual } => write!(
                formatter,
                "archive projection dimension mismatch: expected {expected}, got {actual}",
            ),
        }
    }
}

impl Error for ArchiveError {}

/// A deterministic collection of bounded Pareto fronts across independent
/// descriptor projections.
///
/// Construction is a batch operation so crowding results do not depend on
/// candidate arrival order. A candidate is retained when it survives in at
/// least one projected cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityDiversityArchive<CandidateId, ProjectionId> {
    config: ArchiveConfig,
    candidates: BTreeMap<CandidateId, ArchiveCandidate<CandidateId, ProjectionId>>,
    cells: BTreeMap<ArchiveCellKey<ProjectionId>, ArchiveCell<CandidateId>>,
    elite_cells: BTreeMap<CandidateId, Vec<ArchiveCellKey<ProjectionId>>>,
    diversity_dimensions: usize,
    summary: ArchiveBuildSummary,
}

impl<CandidateId, ProjectionId> QualityDiversityArchive<CandidateId, ProjectionId>
where
    CandidateId: Clone + Ord,
    ProjectionId: Clone + Ord,
{
    /// Build order-independently from precomputed candidates.
    ///
    /// Each cell first drops Pareto-dominated candidates. If its front exceeds
    /// `elites_per_cell`, the least crowded candidate is repeatedly removed.
    /// Crowding is the saturating sum of range-normalized adjacent gaps in
    /// each integer quality coordinate; coordinate extrema are protected.
    /// Equal crowding is broken by retaining the smaller stable candidate
    /// identifier.
    pub fn build(
        config: ArchiveConfig,
        candidates: impl IntoIterator<Item = ArchiveCandidate<CandidateId, ProjectionId>>,
    ) -> Result<Self, ArchiveError> {
        if config.quality_directions.is_empty() {
            return Err(ArchiveError::EmptyQualityVector);
        }
        if config.elites_per_cell == 0 {
            return Err(ArchiveError::ZeroCellCapacity);
        }

        let mut submitted = BTreeMap::new();
        let mut projection_dimensions = BTreeMap::<ProjectionId, usize>::new();
        let mut diversity_dimensions = None;

        for mut candidate in candidates {
            if candidate.quality.len() != config.quality_directions.len() {
                return Err(ArchiveError::QualityDimension {
                    expected: config.quality_directions.len(),
                    actual: candidate.quality.len(),
                });
            }
            if candidate.projections.is_empty() {
                return Err(ArchiveError::CandidateWithoutProjection);
            }
            if candidate.diversity_coordinates.is_empty() {
                return Err(ArchiveError::EmptyDiversityVector);
            }
            if let Some(expected) = diversity_dimensions {
                if candidate.diversity_coordinates.len() != expected {
                    return Err(ArchiveError::DiversityDimension {
                        expected,
                        actual: candidate.diversity_coordinates.len(),
                    });
                }
            } else {
                diversity_dimensions = Some(candidate.diversity_coordinates.len());
            }

            candidate
                .projections
                .sort_by(|left, right| left.projection.cmp(&right.projection));
            if candidate
                .projections
                .windows(2)
                .any(|pair| pair[0].projection == pair[1].projection)
            {
                return Err(ArchiveError::DuplicateProjectionForCandidate);
            }
            for projected in &candidate.projections {
                match projection_dimensions.get(&projected.projection) {
                    Some(&expected) if projected.coordinates.len() != expected => {
                        return Err(ArchiveError::ProjectionDimension {
                            expected,
                            actual: projected.coordinates.len(),
                        });
                    }
                    Some(_) => {}
                    None => {
                        projection_dimensions
                            .insert(projected.projection.clone(), projected.coordinates.len());
                    }
                }
            }

            let id = candidate.id.clone();
            if submitted.insert(id, candidate).is_some() {
                return Err(ArchiveError::DuplicateCandidateId);
            }
        }

        let submitted_candidates = submitted.len();
        let mut memberships = BTreeMap::<ArchiveCellKey<ProjectionId>, Vec<CandidateId>>::new();
        for candidate in submitted.values() {
            for projected in &candidate.projections {
                memberships
                    .entry(ArchiveCellKey {
                        projection: projected.projection.clone(),
                        coordinates: projected.coordinates.clone(),
                    })
                    .or_default()
                    .push(candidate.id.clone());
            }
        }

        let mut cells = BTreeMap::new();
        let mut elite_cells = BTreeMap::<CandidateId, Vec<ArchiveCellKey<ProjectionId>>>::new();
        for (cell_key, members) in memberships {
            let elites = bounded_pareto_front(
                &members,
                &submitted,
                &config.quality_directions,
                config.elites_per_cell,
            );
            for id in &elites {
                elite_cells
                    .entry(id.clone())
                    .or_default()
                    .push(cell_key.clone());
            }
            cells.insert(cell_key, ArchiveCell { elites });
        }

        let retained_candidates = elite_cells.len();
        let elite_placements = elite_cells.values().map(Vec::len).sum();
        submitted.retain(|id, _| elite_cells.contains_key(id));
        let summary = ArchiveBuildSummary {
            submitted_candidates,
            retained_candidates,
            cell_count: cells.len(),
            elite_placements,
        };

        Ok(Self {
            config,
            candidates: submitted,
            cells,
            elite_cells,
            diversity_dimensions: diversity_dimensions.unwrap_or_default(),
            summary,
        })
    }

    #[must_use]
    pub fn config(&self) -> &ArchiveConfig {
        &self.config
    }

    #[must_use]
    pub fn summary(&self) -> ArchiveBuildSummary {
        self.summary
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    #[must_use]
    pub fn diversity_dimensions(&self) -> usize {
        self.diversity_dimensions
    }

    #[must_use]
    pub fn candidate(
        &self,
        id: &CandidateId,
    ) -> Option<&ArchiveCandidate<CandidateId, ProjectionId>> {
        self.candidates.get(id)
    }

    /// Retained candidate identifiers in stable order.
    pub fn candidate_ids(&self) -> impl ExactSizeIterator<Item = &CandidateId> {
        self.candidates.keys()
    }

    pub fn cells(
        &self,
    ) -> impl ExactSizeIterator<Item = (&ArchiveCellKey<ProjectionId>, &ArchiveCell<CandidateId>)>
    {
        self.cells.iter()
    }

    #[must_use]
    pub fn cell(&self, key: &ArchiveCellKey<ProjectionId>) -> Option<&ArchiveCell<CandidateId>> {
        self.cells.get(key)
    }

    /// Cells in which this candidate actually survived Pareto and crowding
    /// reduction. This can be a subset of the candidate's cell assignments.
    #[must_use]
    pub fn elite_cells_for(&self, id: &CandidateId) -> Option<&[ArchiveCellKey<ProjectionId>]> {
        self.elite_cells.get(id).map(Vec::as_slice)
    }

    /// Select singleton candidates with marginal projected-cell coverage as
    /// the primary criterion and max-min L1 diversity as the secondary one.
    pub fn select_farthest(
        &self,
        requested: RangeInclusive<usize>,
    ) -> Result<SelectionResult<CandidateId, CandidateId, ProjectionId>, SelectionError> {
        let packages = self
            .candidate_ids()
            .cloned()
            .map(|id| SelectionPackage::new(id.clone(), vec![id]));
        self.select_farthest_packages(packages, requested, |_| true)
    }

    /// Select atomic caller-defined packages.
    ///
    /// Packages and the `is_eligible` predicate form the seam for later
    /// constraints such as socket closure. This module does not know about or
    /// attempt to enforce such constraints. Package keys and members must be
    /// unique; members are selected atomically. The predicate must be a pure,
    /// deterministic function of its package for the result to be
    /// reproducible.
    ///
    /// Selection greedily prefers (in order): more newly covered cells across
    /// the independent projections, greater minimum L1 distance from the
    /// already selected set, then the smaller stable package key. It selects
    /// while another whole package fits under the requested maximum and
    /// reports an error if the resulting candidate count is below the
    /// requested minimum.
    pub fn select_farthest_packages<PackageId>(
        &self,
        packages: impl IntoIterator<Item = SelectionPackage<PackageId, CandidateId>>,
        requested: RangeInclusive<usize>,
        mut is_eligible: impl FnMut(&SelectionPackage<PackageId, CandidateId>) -> bool,
    ) -> Result<SelectionResult<PackageId, CandidateId, ProjectionId>, SelectionError>
    where
        PackageId: Clone + Ord,
    {
        let minimum = *requested.start();
        let maximum = *requested.end();
        if minimum > maximum {
            return Err(SelectionError::InvalidRequestedRange { minimum, maximum });
        }

        let mut by_key = BTreeMap::<PackageId, SelectionPackage<PackageId, CandidateId>>::new();
        let mut packaged_members = BTreeSet::new();
        for mut package in packages {
            if package.members.is_empty() {
                return Err(SelectionError::EmptyPackage);
            }
            package.members.sort();
            if package.members.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(SelectionError::DuplicatePackageMember);
            }
            if package
                .members
                .iter()
                .any(|member| !self.candidates.contains_key(member))
            {
                return Err(SelectionError::UnknownCandidate);
            }
            if package
                .members
                .iter()
                .any(|member| !packaged_members.insert(member.clone()))
            {
                return Err(SelectionError::CandidateInMultiplePackages);
            }
            if by_key.contains_key(&package.id) {
                return Err(SelectionError::DuplicatePackageId);
            }
            if is_eligible(&package) {
                by_key.insert(package.id.clone(), package);
            }
        }

        let mut remaining = by_key;
        // L1 distance is immutable for the lifetime of the archive.  Cache
        // each active candidate pair once, then maintain each package's
        // minimum distance as the selected set grows.  The previous direct
        // implementation recomputed every old pair at every greedy step,
        // making singleton selection cubic in the number of candidates.
        let active_candidate_ids = remaining
            .values()
            .flat_map(|package| package.members.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let active_candidate_indices = active_candidate_ids
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, id)| (id, index))
            .collect::<BTreeMap<_, _>>();
        let pairwise_distances = PairwiseDistanceCache::new(
            active_candidate_ids
                .iter()
                .map(|id| self.candidates[id].diversity_coordinates.as_slice()),
        );
        let mut minimum_distances = remaining
            .iter()
            .map(|(package_id, package)| {
                (
                    package_id.clone(),
                    package_internal_minimum_distance(
                        &package.members,
                        &active_candidate_indices,
                        &pairwise_distances,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let mut selected_ids = Vec::<CandidateId>::new();
        let mut selected_packages = Vec::<PackageId>::new();
        let mut covered = BTreeSet::<ArchiveCellKey<ProjectionId>>::new();
        let mut steps = Vec::new();

        loop {
            let room = maximum.saturating_sub(selected_ids.len());
            let best = remaining
                .iter()
                .filter(|(_, package)| package.members.len() <= room)
                .map(|(package_id, package)| {
                    let marginal_cell_coverage =
                        self.marginal_cell_coverage(&package.members, &covered);
                    let minimum_l1_distance = minimum_distances[package_id];
                    (package_id, marginal_cell_coverage, minimum_l1_distance)
                })
                .max_by(|left, right| {
                    compare_selection_choice(left.1, left.2, left.0, right.1, right.2, right.0)
                })
                .map(|(package_id, marginal, distance)| (package_id.clone(), marginal, distance));
            let Some((package_id, marginal_cell_coverage, minimum_l1_distance)) = best else {
                break;
            };
            let package = remaining
                .remove(&package_id)
                .expect("selected package came from remaining map");
            minimum_distances.remove(&package_id);
            for (remaining_id, remaining_package) in &remaining {
                let distance_to_new_package = package_cross_minimum_distance(
                    &remaining_package.members,
                    &package.members,
                    &active_candidate_indices,
                    &pairwise_distances,
                );
                let minimum = minimum_distances
                    .get_mut(remaining_id)
                    .expect("every remaining package has a distance state");
                *minimum = minimum_option(*minimum, distance_to_new_package);
            }
            for member in &package.members {
                let candidate = self
                    .candidates
                    .get(member)
                    .expect("package members were validated");
                for projected in &candidate.projections {
                    covered.insert(ArchiveCellKey {
                        projection: projected.projection.clone(),
                        coordinates: projected.coordinates.clone(),
                    });
                }
            }
            selected_ids.extend(package.members.iter().cloned());
            selected_packages.push(package_id.clone());
            steps.push(SelectionStep {
                package_id,
                members: package.members,
                marginal_cell_coverage,
                minimum_l1_distance,
            });
        }

        if selected_ids.len() < minimum {
            return Err(SelectionError::RequestedMinimumUnavailable {
                minimum,
                maximum,
                selected: selected_ids.len(),
            });
        }

        Ok(SelectionResult {
            selected_packages,
            selected_candidates: selected_ids,
            covered_cells: covered.into_iter().collect(),
            steps,
        })
    }

    fn marginal_cell_coverage(
        &self,
        members: &[CandidateId],
        covered: &BTreeSet<ArchiveCellKey<ProjectionId>>,
    ) -> usize {
        members
            .iter()
            .filter_map(|id| self.candidates.get(id))
            .flat_map(|candidate| &candidate.projections)
            .map(|projected| ArchiveCellKey {
                projection: projected.projection.clone(),
                coordinates: projected.coordinates.clone(),
            })
            .filter(|cell| !covered.contains(cell))
            .collect::<BTreeSet<_>>()
            .len()
    }

    #[cfg(test)]
    fn package_minimum_distance_reference(
        &self,
        members: &[CandidateId],
        selected: &[CandidateId],
    ) -> Option<u128> {
        let mut minimum = None;
        for (index, member) in members.iter().enumerate() {
            let candidate = self
                .candidates
                .get(member)
                .expect("package members were validated");
            for other in selected.iter().chain(&members[..index]) {
                let other = self
                    .candidates
                    .get(other)
                    .expect("selected candidates came from the archive");
                let distance = l1_distance(
                    &candidate.diversity_coordinates,
                    &other.diversity_coordinates,
                );
                minimum = Some(minimum.map_or(distance, |old: u128| old.min(distance)));
            }
        }
        minimum
    }

    /// The pre-optimization selector, retained as a differential-test oracle.
    #[cfg(test)]
    fn select_farthest_packages_reference<PackageId>(
        &self,
        packages: impl IntoIterator<Item = SelectionPackage<PackageId, CandidateId>>,
        requested: RangeInclusive<usize>,
        mut is_eligible: impl FnMut(&SelectionPackage<PackageId, CandidateId>) -> bool,
    ) -> Result<SelectionResult<PackageId, CandidateId, ProjectionId>, SelectionError>
    where
        PackageId: Clone + Ord,
    {
        let minimum = *requested.start();
        let maximum = *requested.end();
        if minimum > maximum {
            return Err(SelectionError::InvalidRequestedRange { minimum, maximum });
        }

        let mut by_key = BTreeMap::<PackageId, SelectionPackage<PackageId, CandidateId>>::new();
        let mut packaged_members = BTreeSet::new();
        for mut package in packages {
            if package.members.is_empty() {
                return Err(SelectionError::EmptyPackage);
            }
            package.members.sort();
            if package.members.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(SelectionError::DuplicatePackageMember);
            }
            if package
                .members
                .iter()
                .any(|member| !self.candidates.contains_key(member))
            {
                return Err(SelectionError::UnknownCandidate);
            }
            if package
                .members
                .iter()
                .any(|member| !packaged_members.insert(member.clone()))
            {
                return Err(SelectionError::CandidateInMultiplePackages);
            }
            if by_key.contains_key(&package.id) {
                return Err(SelectionError::DuplicatePackageId);
            }
            if is_eligible(&package) {
                by_key.insert(package.id.clone(), package);
            }
        }

        let mut remaining = by_key;
        let mut selected_ids = Vec::<CandidateId>::new();
        let mut selected_packages = Vec::<PackageId>::new();
        let mut covered = BTreeSet::<ArchiveCellKey<ProjectionId>>::new();
        let mut steps = Vec::new();

        loop {
            let room = maximum.saturating_sub(selected_ids.len());
            let best = remaining
                .iter()
                .filter(|(_, package)| package.members.len() <= room)
                .map(|(package_id, package)| {
                    let marginal_cell_coverage =
                        self.marginal_cell_coverage(&package.members, &covered);
                    let minimum_l1_distance =
                        self.package_minimum_distance_reference(&package.members, &selected_ids);
                    (package_id, marginal_cell_coverage, minimum_l1_distance)
                })
                .max_by(|left, right| {
                    compare_selection_choice(left.1, left.2, left.0, right.1, right.2, right.0)
                })
                .map(|(package_id, marginal, distance)| (package_id.clone(), marginal, distance));
            let Some((package_id, marginal_cell_coverage, minimum_l1_distance)) = best else {
                break;
            };
            let package = remaining
                .remove(&package_id)
                .expect("selected package came from remaining map");
            for member in &package.members {
                let candidate = self
                    .candidates
                    .get(member)
                    .expect("package members were validated");
                for projected in &candidate.projections {
                    covered.insert(ArchiveCellKey {
                        projection: projected.projection.clone(),
                        coordinates: projected.coordinates.clone(),
                    });
                }
            }
            selected_ids.extend(package.members.iter().cloned());
            selected_packages.push(package_id.clone());
            steps.push(SelectionStep {
                package_id,
                members: package.members,
                marginal_cell_coverage,
                minimum_l1_distance,
            });
        }

        if selected_ids.len() < minimum {
            return Err(SelectionError::RequestedMinimumUnavailable {
                minimum,
                maximum,
                selected: selected_ids.len(),
            });
        }

        Ok(SelectionResult {
            selected_packages,
            selected_candidates: selected_ids,
            covered_cells: covered.into_iter().collect(),
            steps,
        })
    }
}

/// Atomic unit offered to package-aware representative selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionPackage<PackageId, CandidateId> {
    pub id: PackageId,
    pub members: Vec<CandidateId>,
}

impl<PackageId, CandidateId> SelectionPackage<PackageId, CandidateId> {
    #[must_use]
    pub fn new(id: PackageId, members: Vec<CandidateId>) -> Self {
        Self { id, members }
    }
}

/// One auditable greedy choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionStep<PackageId, CandidateId> {
    pub package_id: PackageId,
    pub members: Vec<CandidateId>,
    pub marginal_cell_coverage: usize,
    /// `None` means there was no previously or internally selected point from
    /// which to measure distance (normally the first singleton choice).
    pub minimum_l1_distance: Option<u128>,
}

/// Deterministic representative selection and its rationale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionResult<PackageId, CandidateId, ProjectionId> {
    /// Package identifiers in greedy selection order.
    pub selected_packages: Vec<PackageId>,
    /// Candidate identifiers in package-selection order and stable order
    /// within each package.
    pub selected_candidates: Vec<CandidateId>,
    /// Covered independent projected cells in stable key order.
    pub covered_cells: Vec<ArchiveCellKey<ProjectionId>>,
    pub steps: Vec<SelectionStep<PackageId, CandidateId>>,
}

/// Invalid selection request or package input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectionError {
    InvalidRequestedRange {
        minimum: usize,
        maximum: usize,
    },
    EmptyPackage,
    DuplicatePackageId,
    DuplicatePackageMember,
    CandidateInMultiplePackages,
    UnknownCandidate,
    RequestedMinimumUnavailable {
        minimum: usize,
        maximum: usize,
        selected: usize,
    },
}

impl fmt::Display for SelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequestedRange { minimum, maximum } => {
                write!(formatter, "invalid selection range {minimum}..={maximum}",)
            }
            Self::EmptyPackage => formatter.write_str("selection package is empty"),
            Self::DuplicatePackageId => {
                formatter.write_str("selection package identifiers must be unique")
            }
            Self::DuplicatePackageMember => {
                formatter.write_str("selection package contains a candidate more than once")
            }
            Self::CandidateInMultiplePackages => {
                formatter.write_str("archive candidate belongs to more than one selection package")
            }
            Self::UnknownCandidate => {
                formatter.write_str("selection package contains a candidate absent from archive")
            }
            Self::RequestedMinimumUnavailable {
                minimum,
                maximum,
                selected,
            } => write!(
                formatter,
                "selection could retain only {selected} candidates for requested range {minimum}..={maximum}",
            ),
        }
    }
}

impl Error for SelectionError {}

fn bounded_pareto_front<CandidateId, ProjectionId>(
    members: &[CandidateId],
    candidates: &BTreeMap<CandidateId, ArchiveCandidate<CandidateId, ProjectionId>>,
    directions: &[ObjectiveDirection],
    capacity: usize,
) -> Vec<CandidateId>
where
    CandidateId: Clone + Ord,
{
    let mut front = members
        .iter()
        .filter(|candidate_id| {
            let candidate = &candidates[*candidate_id];
            !members.iter().any(|other_id| {
                other_id != *candidate_id
                    && dominates(
                        &candidates[other_id].quality,
                        &candidate.quality,
                        directions,
                    )
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    front.sort();

    while front.len() > capacity {
        let crowding = crowding_scores(&front, candidates);
        let remove = (0..front.len())
            .min_by(|&left, &right| {
                crowding[left]
                    .cmp(&crowding[right])
                    // On equal crowding, the larger stable id is evicted.
                    .then_with(|| front[right].cmp(&front[left]))
            })
            .expect("over-capacity front is non-empty");
        front.remove(remove);
    }
    front
}

fn dominates(left: &[i64], right: &[i64], directions: &[ObjectiveDirection]) -> bool {
    let mut strictly_better = false;
    for ((left, right), direction) in left.iter().zip(right).zip(directions) {
        let ordering = match direction {
            ObjectiveDirection::Maximize => left.cmp(right),
            ObjectiveDirection::Minimize => right.cmp(left),
        };
        if ordering == Ordering::Less {
            return false;
        }
        strictly_better |= ordering == Ordering::Greater;
    }
    strictly_better
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CrowdingScore {
    Finite(u128),
    Boundary,
}

fn crowding_scores<CandidateId, ProjectionId>(
    front: &[CandidateId],
    candidates: &BTreeMap<CandidateId, ArchiveCandidate<CandidateId, ProjectionId>>,
) -> Vec<CrowdingScore>
where
    CandidateId: Ord,
{
    let dimensions = candidates[&front[0]].quality.len();
    let mut scores = vec![CrowdingScore::Finite(0); front.len()];
    for dimension in 0..dimensions {
        let mut order = (0..front.len()).collect::<Vec<_>>();
        order.sort_by(|&left, &right| {
            candidates[&front[left]].quality[dimension]
                .cmp(&candidates[&front[right]].quality[dimension])
                .then_with(|| front[left].cmp(&front[right]))
        });
        let minimum = candidates[&front[order[0]]].quality[dimension];
        let maximum =
            candidates[&front[*order.last().expect("front is non-empty")]].quality[dimension];
        let range = minimum.abs_diff(maximum);
        if range == 0 {
            // A constant coordinate provides neither an extreme nor spacing
            // evidence and must not turn the entire front into boundaries.
            continue;
        }
        for &index in &order {
            let value = candidates[&front[index]].quality[dimension];
            if value == minimum || value == maximum {
                scores[index] = CrowdingScore::Boundary;
            }
        }
        for window in order.windows(3) {
            let index = window[1];
            if scores[index] == CrowdingScore::Boundary {
                continue;
            }
            let lower = candidates[&front[window[0]]].quality[dimension];
            let upper = candidates[&front[window[2]]].quality[dimension];
            let gap = u128::from(lower.abs_diff(upper));
            // Fixed-point normalization is deterministic and prevents the
            // raw unit scale of one quality coordinate from controlling cap
            // eviction. The multiplication fits because both operands are at
            // most u64::MAX.
            let normalized = gap
                .saturating_mul(u128::from(u64::MAX))
                .saturating_add(u128::from(range / 2))
                / u128::from(range);
            if let CrowdingScore::Finite(score) = &mut scores[index] {
                *score = score.saturating_add(normalized);
            }
        }
    }
    scores
}

fn l1_distance(left: &[i64], right: &[i64]) -> u128 {
    left.iter()
        .zip(right)
        .map(|(left, right)| u128::from(left.abs_diff(*right)))
        .fold(0_u128, u128::saturating_add)
}

/// Compact lower-triangular cache of distances between distinct candidates.
/// Row `high` contains distances `(high, 0)..(high, high - 1)`.
struct PairwiseDistanceCache {
    candidate_count: usize,
    distances: Vec<u128>,
}

impl PairwiseDistanceCache {
    fn new<'a>(coordinates: impl IntoIterator<Item = &'a [i64]>) -> Self {
        let coordinates = coordinates.into_iter().collect::<Vec<_>>();
        let candidate_count = coordinates.len();
        let mut distances = Vec::with_capacity(
            candidate_count.saturating_mul(candidate_count.saturating_sub(1)) / 2,
        );
        for high in 0..candidate_count {
            for low in 0..high {
                distances.push(l1_distance(coordinates[high], coordinates[low]));
            }
        }
        Self {
            candidate_count,
            distances,
        }
    }

    fn get(&self, left: usize, right: usize) -> u128 {
        assert!(left < self.candidate_count && right < self.candidate_count);
        assert_ne!(left, right, "packages cannot share a candidate");
        let (low, high) = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        self.distances[high * (high - 1) / 2 + low]
    }
}

fn package_internal_minimum_distance<CandidateId: Ord>(
    members: &[CandidateId],
    candidate_indices: &BTreeMap<CandidateId, usize>,
    distances: &PairwiseDistanceCache,
) -> Option<u128> {
    let mut minimum = None;
    for high in 1..members.len() {
        for low in 0..high {
            let distance = distances.get(
                candidate_indices[&members[high]],
                candidate_indices[&members[low]],
            );
            minimum = minimum_option(minimum, Some(distance));
        }
    }
    minimum
}

fn package_cross_minimum_distance<CandidateId: Ord>(
    left: &[CandidateId],
    right: &[CandidateId],
    candidate_indices: &BTreeMap<CandidateId, usize>,
    distances: &PairwiseDistanceCache,
) -> Option<u128> {
    let mut minimum = None;
    for left_member in left {
        for right_member in right {
            let distance = distances.get(
                candidate_indices[left_member],
                candidate_indices[right_member],
            );
            minimum = minimum_option(minimum, Some(distance));
        }
    }
    minimum
}

fn minimum_option(left: Option<u128>, right: Option<u128>) -> Option<u128> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

fn compare_selection_choice<PackageId: Ord>(
    left_coverage: usize,
    left_distance: Option<u128>,
    left_id: &PackageId,
    right_coverage: usize,
    right_distance: Option<u128>,
    right_id: &PackageId,
) -> Ordering {
    left_coverage
        .cmp(&right_coverage)
        .then_with(|| match (left_distance, right_distance) {
            (Some(left), Some(right)) => left.cmp(&right),
            // A first singleton has no pairwise distance constraint, which is
            // the max-min analogue of an unbounded distance. A multi-member
            // package does have an internal distance.
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (None, None) => Ordering::Equal,
        })
        // max_by must prefer the smaller stable identifier.
        .then_with(|| right_id.cmp(left_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projected(projection: u8, cell: i64) -> ProjectedCell<u8> {
        ProjectedCell::new(projection, vec![cell])
    }

    fn candidate(
        id: u8,
        projections: &[(u8, i64)],
        quality: &[i64],
        diversity: &[i64],
    ) -> ArchiveCandidate<u8, u8> {
        ArchiveCandidate::new(
            id,
            projections
                .iter()
                .map(|&(projection, cell)| projected(projection, cell))
                .collect(),
            quality.to_vec(),
            diversity.to_vec(),
        )
    }

    fn archive(
        cap: usize,
        candidates: Vec<ArchiveCandidate<u8, u8>>,
    ) -> QualityDiversityArchive<u8, u8> {
        QualityDiversityArchive::build(
            ArchiveConfig::new(
                vec![ObjectiveDirection::Maximize, ObjectiveDirection::Maximize],
                cap,
            ),
            candidates,
        )
        .unwrap()
    }

    #[test]
    fn dominated_candidate_is_rejected() {
        let archive = archive(
            4,
            vec![
                candidate(1, &[(0, 0)], &[3, 3], &[0]),
                candidate(2, &[(0, 0)], &[2, 3], &[1]),
            ],
        );

        assert!(archive.candidate(&1).is_some());
        assert!(archive.candidate(&2).is_none());
        assert_eq!(archive.cells().next().unwrap().1.elites(), &[1],);
    }

    #[test]
    fn pareto_incomparable_candidates_are_kept() {
        let archive = archive(
            4,
            vec![
                candidate(1, &[(0, 0)], &[5, 2], &[0]),
                candidate(2, &[(0, 0)], &[2, 5], &[1]),
            ],
        );

        assert_eq!(archive.cells().next().unwrap().1.elites(), &[1, 2]);
    }

    #[test]
    fn cell_cap_and_crowding_tie_break_are_input_order_independent() {
        let inputs = vec![
            candidate(4, &[(0, 0)], &[0, 30], &[4]),
            candidate(3, &[(0, 0)], &[10, 20], &[3]),
            candidate(2, &[(0, 0)], &[20, 10], &[2]),
            candidate(1, &[(0, 0)], &[30, 0], &[1]),
        ];
        let mut reversed = inputs.clone();
        reversed.reverse();

        let first = archive(3, inputs);
        let second = archive(3, reversed);

        // Both extremes survive. The equal-crowding interior tie keeps the
        // smaller stable identifier.
        assert_eq!(first.cells().next().unwrap().1.elites(), &[1, 2, 4]);
        assert_eq!(first, second);
    }

    #[test]
    fn marginal_coverage_counts_independent_projection_cells() {
        let archive = archive(
            4,
            vec![
                candidate(1, &[(0, 0), (1, 0)], &[3, 3], &[0]),
                candidate(2, &[(0, 0), (1, 1)], &[3, 3], &[1]),
                candidate(3, &[(0, 1), (1, 1)], &[3, 3], &[2]),
            ],
        );

        let selected = archive.select_farthest(2..=2).unwrap();

        assert_eq!(selected.selected_candidates, vec![1, 3]);
        assert_eq!(selected.steps[0].marginal_cell_coverage, 2);
        assert_eq!(selected.steps[1].marginal_cell_coverage, 2);
        assert_eq!(selected.covered_cells.len(), 4);
        // There are four independent cells, not three Cartesian tuple cells.
        assert_eq!(archive.summary().cell_count, 4);
    }

    #[test]
    fn farthest_point_tie_uses_stable_candidate_id() {
        let archive = archive(
            4,
            vec![
                candidate(1, &[(0, 0)], &[3, 3], &[0]),
                candidate(2, &[(0, 0)], &[3, 3], &[-10]),
                candidate(3, &[(0, 0)], &[3, 3], &[10]),
            ],
        );

        let selected = archive.select_farthest(2..=2).unwrap();

        assert_eq!(selected.selected_candidates, vec![1, 2]);
        assert_eq!(selected.steps[1].minimum_l1_distance, Some(10));
    }

    #[test]
    fn cached_package_selection_matches_reference_across_internal_and_external_ties() {
        let archive = archive(
            16,
            vec![
                candidate(0, &[(0, 0), (1, 0)], &[3, 3], &[0, 0]),
                candidate(1, &[(0, 0), (1, 1)], &[3, 3], &[0, 10]),
                candidate(2, &[(0, 1), (1, 0)], &[3, 3], &[10, 0]),
                candidate(3, &[(0, 1), (1, 1)], &[3, 3], &[10, 10]),
                candidate(4, &[(0, 2), (1, 0)], &[3, 3], &[-10, 0]),
                candidate(5, &[(0, 2), (1, 1)], &[3, 3], &[0, -10]),
            ],
        );
        let packages = vec![
            SelectionPackage::new(20, vec![1, 0]),
            SelectionPackage::new(10, vec![2]),
            SelectionPackage::new(30, vec![4, 3]),
            SelectionPackage::new(40, vec![5]),
        ];

        let cached = archive
            .select_farthest_packages(packages.clone(), 3..=5, |_| true)
            .unwrap();
        let reference = archive
            .select_farthest_packages_reference(packages, 3..=5, |_| true)
            .unwrap();

        assert_eq!(cached, reference);
    }

    #[test]
    fn cached_selection_matches_reference_on_deterministic_random_fixtures() {
        let mut state = 0x9e37_79b9_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state
        };

        for fixture in 0..48_u8 {
            let candidate_count = 12 + (next() % 13) as u8;
            let mut candidates = Vec::new();
            for id in 0..candidate_count {
                let diversity = (0..7)
                    .map(|dimension| {
                        let value = next();
                        // Small ranges deliberately create exact distance
                        // ties; occasional extrema exercise saturating L1.
                        if id == 0 && dimension == 0 {
                            i64::MIN
                        } else if id == 1 && dimension == 0 {
                            i64::MAX
                        } else {
                            (value % 11) as i64 - 5
                        }
                    })
                    .collect::<Vec<_>>();
                candidates.push(candidate(
                    id,
                    &[(0, i64::from(id % 4)), (1, i64::from((id + fixture) % 5))],
                    &[1, 1],
                    &diversity,
                ));
            }
            let archive = archive(32, candidates);

            let mut packages = Vec::new();
            let mut member = 0_u8;
            let mut package_id = 0_u8;
            while member < candidate_count {
                let package_len = 1 + (next() % 3) as u8;
                let end = member.saturating_add(package_len).min(candidate_count);
                packages.push(SelectionPackage::new(
                    package_id,
                    (member..end).rev().collect(),
                ));
                member = end;
                package_id += 1;
            }
            let maximum = (next() % u64::from(candidate_count + 1)) as usize;
            let eligibility_modulus = 2 + (next() % 3) as u8;
            let eligible = |package: &SelectionPackage<u8, u8>| {
                package.id.wrapping_add(fixture) % eligibility_modulus != 0
            };

            let cached = archive.select_farthest_packages(packages.clone(), 0..=maximum, eligible);
            let reference =
                archive.select_farthest_packages_reference(packages, 0..=maximum, eligible);
            assert_eq!(cached, reference, "fixture {fixture}");
        }
    }

    #[test]
    fn quality_is_not_collapsed_to_a_scalar_fun_score() {
        let archive = archive(
            4,
            vec![
                // A weighted sum could rank one of these above the other;
                // Pareto comparison correctly preserves the trade-off.
                candidate(1, &[(0, 0)], &[100, 0], &[0]),
                candidate(2, &[(0, 0)], &[40, 40], &[1]),
            ],
        );

        assert_eq!(archive.cells().next().unwrap().1.elites(), &[1, 2]);
    }
}
