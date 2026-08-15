//! Deterministic socket-closed units for corpus selection.
//!
//! The quality-diversity selector should not have to repair socket closure
//! after choosing individual rooms.  This module first finds a room set in a
//! requested size range, then realizes a one-to-one matching of every socket
//! with an opposite socket.  Connected components of that matching are the
//! returned atomic packages.  Consequently, selecting any union of complete
//! packages preserves socket closure.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    error::Error,
    fmt,
    hash::{Hash, Hasher},
};

use downwards_core::DoorSocket;

/// Default limit for the deterministic closure search.
pub const DEFAULT_SOCKET_PACKAGE_NODE_BUDGET: usize = 1_000_000;

/// A room offered to socket packaging.
///
/// `sockets` is a multiset: repeated values represent distinct apertures and
/// must each receive a distinct mate.  Stable IDs must be unique across the
/// complete input, including ineligible rooms.
#[derive(Clone, Copy, Debug)]
pub struct SocketPackageRoom<'a, Id> {
    pub stable_id: &'a Id,
    pub sockets: &'a [DoorSocket],
    pub eligible: bool,
}

/// Whether two sockets belonging to one room may be paired.
///
/// Dungeon assembly normally joins different rooms, so the default is the
/// strict `OtherSelectedRoom` policy.  `AllowSameRoom` is an explicit escape
/// hatch for a future assembly design that gives such pairs a real meaning.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SocketClosurePolicy {
    #[default]
    OtherSelectedRoom,
    AllowSameRoom,
}

/// Bounds and resource limit for one packaging run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketPackageRequest {
    pub target_min_rooms: usize,
    pub target_max_rooms: usize,
    pub node_budget: usize,
    pub closure_policy: SocketClosurePolicy,
}

impl SocketPackageRequest {
    /// Construct a request using strict cross-room matching and the default
    /// search budget.
    #[must_use]
    pub const fn new(target_min_rooms: usize, target_max_rooms: usize) -> Self {
        Self {
            target_min_rooms,
            target_max_rooms,
            node_budget: DEFAULT_SOCKET_PACKAGE_NODE_BUDGET,
            closure_policy: SocketClosurePolicy::OtherSelectedRoom,
        }
    }
}

impl Default for SocketPackageRequest {
    fn default() -> Self {
        Self::new(500, 1_000)
    }
}

/// One independently socket-closed selection unit.
///
/// IDs are in stable-ID order.  Packages themselves are ordered by their
/// first ID, so the complete result is independent of input ordering and
/// socket ordering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketPackage<Id> {
    pub room_ids: Vec<Id>,
}

/// A deterministic package plan within the requested inclusive room range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketPackagePlan<Id> {
    pub packages: Vec<SocketPackage<Id>>,
    pub total_rooms: usize,
    pub explored_nodes: usize,
}

/// One socket occurrence class for which no other eligible room provides a
/// compatible mate. This is the corpus-level reuse/assembly coverage check;
/// it is intentionally weaker than consuming every selected room exactly
/// once in a balanced pairing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UncoveredSocket<Id> {
    pub room_id: Id,
    pub socket: DoorSocket,
    pub occurrences: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SocketMateCoverageReport<Id> {
    pub eligible_rooms: usize,
    pub eligible_socket_occurrences: usize,
    pub covered_socket_occurrences: usize,
    pub rooms_with_complete_mate_coverage: usize,
    pub uncovered: Vec<UncoveredSocket<Id>>,
}

impl<Id> SocketMateCoverageReport<Id> {
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.uncovered.is_empty()
            && self.covered_socket_occurrences == self.eligible_socket_occurrences
            && self.rooms_with_complete_mate_coverage == self.eligible_rooms
    }
}

/// Failure to construct a socket-closed plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketPackageError {
    InvalidTargetRange {
        minimum: usize,
        maximum: usize,
    },
    DuplicateStableId {
        first_input_index: usize,
        duplicate_input_index: usize,
    },
    /// The complete admissible search space was explored without a plan.
    Exhausted {
        minimum: usize,
        maximum: usize,
        eligible_rooms: usize,
        explored_nodes: usize,
    },
    /// The resource bound was reached; impossibility has not been proved.
    Inconclusive {
        minimum: usize,
        maximum: usize,
        node_budget: usize,
        explored_nodes: usize,
    },
    /// Search claimed closure but deterministic socket pairing could not
    /// realize it.  This indicates an implementation invariant failure.
    InternalInvariant,
}

impl fmt::Display for SocketPackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTargetRange { minimum, maximum } => write!(
                formatter,
                "socket-package target range must satisfy 0 < minimum <= maximum, got {minimum}..={maximum}"
            ),
            Self::DuplicateStableId {
                first_input_index,
                duplicate_input_index,
            } => write!(
                formatter,
                "socket-package inputs {first_input_index} and {duplicate_input_index} have the same stable ID"
            ),
            Self::Exhausted {
                minimum,
                maximum,
                eligible_rooms,
                explored_nodes,
            } => write!(
                formatter,
                "socket-package search exhausted after {explored_nodes} nodes: no closed selection of {minimum}..={maximum} rooms exists among {eligible_rooms} eligible rooms"
            ),
            Self::Inconclusive {
                minimum,
                maximum,
                node_budget,
                explored_nodes,
            } => write!(
                formatter,
                "socket-package search is inconclusive after {explored_nodes} nodes (budget {node_budget}) for target {minimum}..={maximum}"
            ),
            Self::InternalInvariant => write!(
                formatter,
                "socket-package closure passed but its one-to-one pairing could not be realized"
            ),
        }
    }
}

impl Error for SocketPackageError {}

/// Find deterministic, disjoint, independently socket-closed packages.
///
/// The result contains between `target_min_rooms` and `target_max_rooms`
/// inclusive; it deliberately does not require an exact target.  Search
/// branches first on the unmatched socket with the fewest indexed mate rooms,
/// and equal choices use stable-ID order.  Exhausting `node_budget` returns
/// [`SocketPackageError::Inconclusive`] rather than weakening closure.
pub fn build_socket_packages<Id>(
    rooms: &[SocketPackageRoom<'_, Id>],
    request: SocketPackageRequest,
) -> Result<SocketPackagePlan<Id>, SocketPackageError>
where
    Id: Clone + Ord,
{
    if request.target_min_rooms == 0 || request.target_min_rooms > request.target_max_rooms {
        return Err(SocketPackageError::InvalidTargetRange {
            minimum: request.target_min_rooms,
            maximum: request.target_max_rooms,
        });
    }

    let mut stable_order = (0..rooms.len()).collect::<Vec<_>>();
    stable_order.sort_unstable_by(|&left, &right| {
        rooms[left]
            .stable_id
            .cmp(rooms[right].stable_id)
            .then_with(|| left.cmp(&right))
    });
    for pair in stable_order.windows(2) {
        let [left, right] = pair else {
            unreachable!("windows of two always have two elements");
        };
        if rooms[*left].stable_id == rooms[*right].stable_id {
            return Err(SocketPackageError::DuplicateStableId {
                first_input_index: (*left).min(*right),
                duplicate_input_index: (*left).max(*right),
            });
        }
    }

    let eligible = stable_order
        .into_iter()
        .filter(|&index| rooms[index].eligible)
        .map(|input_index| IndexedRoom::new(input_index, rooms[input_index].sockets))
        .collect::<Vec<_>>();
    if eligible.len() < request.target_min_rooms {
        return Err(SocketPackageError::Exhausted {
            minimum: request.target_min_rooms,
            maximum: request.target_max_rooms,
            eligible_rooms: eligible.len(),
            explored_nodes: 0,
        });
    }

    // The common corpus case deliberately allows the complete eligible pool
    // as the upper bound. Check that inventory before entering subset search:
    // if it is already closed, its realized pairing components are precisely
    // the atomic packages needed by downstream diversity selection.
    if request.node_budget > 0 && eligible.len() <= request.target_max_rooms {
        let selected = (0..eligible.len()).collect::<Vec<_>>();
        if closure_constraints(&eligible, &selected, request.closure_policy).is_empty() {
            let components = pairing_components(&eligible, &selected, request.closure_policy)
                .ok_or(SocketPackageError::InternalInvariant)?;
            let packages = components
                .into_iter()
                .map(|component| SocketPackage {
                    room_ids: component
                        .into_iter()
                        .map(|room_index| rooms[eligible[room_index].input_index].stable_id.clone())
                        .collect(),
                })
                .collect::<Vec<_>>();
            return Ok(SocketPackagePlan {
                packages,
                total_rooms: eligible.len(),
                explored_nodes: 1,
            });
        }
    }

    let mut search = ClosureSearch::new(&eligible, request);
    let selected = match search.run() {
        SearchResult::Found(selected) => selected,
        SearchResult::Exhausted => {
            return Err(SocketPackageError::Exhausted {
                minimum: request.target_min_rooms,
                maximum: request.target_max_rooms,
                eligible_rooms: eligible.len(),
                explored_nodes: search.explored_nodes,
            });
        }
        SearchResult::Inconclusive => {
            return Err(SocketPackageError::Inconclusive {
                minimum: request.target_min_rooms,
                maximum: request.target_max_rooms,
                node_budget: request.node_budget,
                explored_nodes: search.explored_nodes,
            });
        }
    };

    let components = pairing_components(&eligible, &selected, request.closure_policy)
        .ok_or(SocketPackageError::InternalInvariant)?;
    let packages = components
        .into_iter()
        .map(|component| SocketPackage {
            room_ids: component
                .into_iter()
                .map(|room_index| rooms[eligible[room_index].input_index].stable_id.clone())
                .collect(),
        })
        .collect::<Vec<_>>();
    let total_rooms = packages.iter().map(|package| package.room_ids.len()).sum();

    Ok(SocketPackagePlan {
        packages,
        total_rooms,
        explored_nodes: search.explored_nodes,
    })
}

/// Check that every eligible socket has a compatible mate on another
/// eligible room.
///
/// This is the appropriate corpus gate when a later dungeon assembler may
/// reuse room definitions. It proves mate *coverage*, not that one copy of
/// every room can be consumed simultaneously. Use [`build_socket_packages`]
/// for the stronger no-reuse, multiplicity-balanced experiment.
pub fn audit_socket_mate_coverage<Id>(
    rooms: &[SocketPackageRoom<'_, Id>],
) -> Result<SocketMateCoverageReport<Id>, SocketPackageError>
where
    Id: Clone + Ord,
{
    let mut stable_order = (0..rooms.len()).collect::<Vec<_>>();
    stable_order.sort_unstable_by(|&left, &right| {
        rooms[left]
            .stable_id
            .cmp(rooms[right].stable_id)
            .then_with(|| left.cmp(&right))
    });
    for pair in stable_order.windows(2) {
        if rooms[pair[0]].stable_id == rooms[pair[1]].stable_id {
            return Err(SocketPackageError::DuplicateStableId {
                first_input_index: pair[0].min(pair[1]),
                duplicate_input_index: pair[0].max(pair[1]),
            });
        }
    }

    let mut providers = BTreeMap::<DoorSocket, BTreeSet<&Id>>::new();
    for room in rooms.iter().filter(|room| room.eligible) {
        for &socket in room.sockets {
            providers.entry(socket).or_default().insert(room.stable_id);
        }
    }

    let mut uncovered_counts = BTreeMap::<(Id, DoorSocket), usize>::new();
    let mut eligible_rooms = 0;
    let mut eligible_socket_occurrences = 0;
    let mut covered_socket_occurrences = 0;
    let mut rooms_with_complete_mate_coverage = 0;
    for room in rooms.iter().filter(|room| room.eligible) {
        eligible_rooms += 1;
        let mut room_complete = true;
        for &socket in room.sockets {
            eligible_socket_occurrences += 1;
            let covered = providers
                .get(&socket.mate())
                .is_some_and(|room_ids| room_ids.iter().any(|id| *id != room.stable_id));
            if covered {
                covered_socket_occurrences += 1;
            } else {
                room_complete = false;
                *uncovered_counts
                    .entry((room.stable_id.clone(), socket))
                    .or_default() += 1;
            }
        }
        if room_complete {
            rooms_with_complete_mate_coverage += 1;
        }
    }
    let uncovered = uncovered_counts
        .into_iter()
        .map(|((room_id, socket), occurrences)| UncoveredSocket {
            room_id,
            socket,
            occurrences,
        })
        .collect();
    Ok(SocketMateCoverageReport {
        eligible_rooms,
        eligible_socket_occurrences,
        covered_socket_occurrences,
        rooms_with_complete_mate_coverage,
        uncovered,
    })
}

/// Check one-to-one mate closure for a collection of room socket multisets.
///
/// Unlike an existence check based on `any`, this accounts for repeated
/// sockets.  Under the default policy, a socket's mate must belong to a
/// different room.
#[must_use]
pub fn is_pairwise_mate_closed<'a>(
    room_socket_multisets: impl IntoIterator<Item = &'a [DoorSocket]>,
    policy: SocketClosurePolicy,
) -> bool {
    let rooms = room_socket_multisets
        .into_iter()
        .enumerate()
        .map(|(input_index, sockets)| IndexedRoom::new(input_index, sockets))
        .collect::<Vec<_>>();
    let selected = (0..rooms.len()).collect::<Vec<_>>();
    closure_constraints(&rooms, &selected, policy).is_empty()
}

#[derive(Clone, Debug)]
struct IndexedRoom {
    input_index: usize,
    socket_counts: BTreeMap<DoorSocket, usize>,
}

impl IndexedRoom {
    fn new(input_index: usize, sockets: &[DoorSocket]) -> Self {
        let mut socket_counts = BTreeMap::new();
        for &socket in sockets {
            *socket_counts.entry(socket).or_default() += 1;
        }
        Self {
            input_index,
            socket_counts,
        }
    }

    fn socket_count(&self, socket: DoorSocket) -> usize {
        self.socket_counts.get(&socket).copied().unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BitSet {
    words: Vec<u64>,
}

impl BitSet {
    fn new(bits: usize) -> Self {
        Self {
            words: vec![0; bits.div_ceil(u64::BITS as usize)],
        }
    }

    fn contains(&self, index: usize) -> bool {
        self.words[index / u64::BITS as usize] & (1 << (index % u64::BITS as usize)) != 0
    }

    fn insert(&mut self, index: usize) {
        self.words[index / u64::BITS as usize] |= 1 << (index % u64::BITS as usize);
    }

    fn remove(&mut self, index: usize) {
        self.words[index / u64::BITS as usize] &= !(1 << (index % u64::BITS as usize));
    }
}

impl Hash for BitSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.words.hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct SearchState {
    selected: BitSet,
    excluded: BitSet,
}

enum SearchResult {
    Found(Vec<usize>),
    Exhausted,
    Inconclusive,
}

struct ClosureSearch<'a> {
    rooms: &'a [IndexedRoom],
    request: SocketPackageRequest,
    mate_index: BTreeMap<DoorSocket, Vec<usize>>,
    selected: BitSet,
    excluded: BitSet,
    selected_indices: BTreeSet<usize>,
    selected_count: usize,
    excluded_count: usize,
    explored_nodes: usize,
    exhausted_states: HashSet<SearchState>,
}

impl<'a> ClosureSearch<'a> {
    fn new(rooms: &'a [IndexedRoom], request: SocketPackageRequest) -> Self {
        let mut mate_index = BTreeMap::<DoorSocket, Vec<usize>>::new();
        for (room_index, room) in rooms.iter().enumerate() {
            for &socket in room.socket_counts.keys() {
                mate_index.entry(socket).or_default().push(room_index);
            }
        }
        Self {
            rooms,
            request,
            mate_index,
            selected: BitSet::new(rooms.len()),
            excluded: BitSet::new(rooms.len()),
            selected_indices: BTreeSet::new(),
            selected_count: 0,
            excluded_count: 0,
            explored_nodes: 0,
            exhausted_states: HashSet::new(),
        }
    }

    fn run(&mut self) -> SearchResult {
        self.visit()
    }

    fn visit(&mut self) -> SearchResult {
        let state = SearchState {
            selected: self.selected.clone(),
            excluded: self.excluded.clone(),
        };
        if self.exhausted_states.contains(&state) {
            return SearchResult::Exhausted;
        }
        if self.explored_nodes >= self.request.node_budget {
            return SearchResult::Inconclusive;
        }
        self.explored_nodes += 1;

        if self.selected_count > self.request.target_max_rooms
            || self.selected_count + self.rooms.len() - self.selected_count - self.excluded_count
                < self.request.target_min_rooms
        {
            self.exhausted_states.insert(state);
            return SearchResult::Exhausted;
        }

        let selected = self.selected_indices.iter().copied().collect::<Vec<_>>();
        let constraints = closure_constraints(self.rooms, &selected, self.request.closure_policy);
        if constraints.is_empty() {
            if self.selected_count >= self.request.target_min_rooms {
                return SearchResult::Found(selected);
            }

            let Some(seed) = (0..self.rooms.len())
                .find(|&index| !self.selected.contains(index) && !self.excluded.contains(index))
            else {
                self.exhausted_states.insert(state);
                return SearchResult::Exhausted;
            };

            self.select(seed);
            let included = self.visit();
            self.unselect(seed);
            match included {
                SearchResult::Found(_) | SearchResult::Inconclusive => return included,
                SearchResult::Exhausted => {}
            }

            self.exclude(seed);
            let excluded = self.visit();
            self.unexclude(seed);
            if matches!(excluded, SearchResult::Exhausted) {
                self.exhausted_states.insert(state);
            }
            return excluded;
        }

        if self.selected_count == self.request.target_max_rooms {
            self.exhausted_states.insert(state);
            return SearchResult::Exhausted;
        }

        let Some(domain) = constraints
            .into_iter()
            .map(|constraint| {
                let candidates = self
                    .mate_index
                    .get(&constraint.socket.mate())
                    .into_iter()
                    .flatten()
                    .copied()
                    .filter(|&candidate| {
                        !self.selected.contains(candidate) && !self.excluded.contains(candidate)
                    })
                    .collect::<Vec<_>>();
                (constraint, candidates)
            })
            .min_by(|(left_constraint, left), (right_constraint, right)| {
                left.len()
                    .cmp(&right.len())
                    .then_with(|| left_constraint.cmp(right_constraint))
            })
            .map(|(_, candidates)| candidates)
        else {
            unreachable!("a non-closed inventory has at least one constraint");
        };

        if domain.is_empty() {
            self.exhausted_states.insert(state);
            return SearchResult::Exhausted;
        }
        for candidate in domain {
            self.select(candidate);
            let result = self.visit();
            self.unselect(candidate);
            match result {
                SearchResult::Found(_) | SearchResult::Inconclusive => return result,
                SearchResult::Exhausted => {}
            }
        }

        self.exhausted_states.insert(state);
        SearchResult::Exhausted
    }

    fn select(&mut self, room_index: usize) {
        debug_assert!(!self.selected.contains(room_index));
        debug_assert!(!self.excluded.contains(room_index));
        self.selected.insert(room_index);
        self.selected_indices.insert(room_index);
        self.selected_count += 1;
    }

    fn unselect(&mut self, room_index: usize) {
        self.selected.remove(room_index);
        self.selected_indices.remove(&room_index);
        self.selected_count -= 1;
    }

    fn exclude(&mut self, room_index: usize) {
        debug_assert!(!self.selected.contains(room_index));
        debug_assert!(!self.excluded.contains(room_index));
        self.excluded.insert(room_index);
        self.excluded_count += 1;
    }

    fn unexclude(&mut self, room_index: usize) {
        self.excluded.remove(room_index);
        self.excluded_count -= 1;
    }
}

/// One selected socket that still necessarily needs a mate from an additional
/// room.  Ordering provides a deterministic tie break after MRV domain size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ClosureConstraint {
    socket: DoorSocket,
}

fn closure_constraints(
    rooms: &[IndexedRoom],
    selected: &[usize],
    policy: SocketClosurePolicy,
) -> Vec<ClosureConstraint> {
    let mut inventory = BTreeMap::<DoorSocket, usize>::new();
    for &room_index in selected {
        for (&socket, &count) in &rooms[room_index].socket_counts {
            *inventory.entry(socket).or_default() += count;
        }
    }

    let signatures = inventory
        .keys()
        .map(|&socket| socket.min(socket.mate()))
        .collect::<BTreeSet<_>>();
    let mut constraints = BTreeSet::new();
    for socket in signatures {
        let mate = socket.mate();
        let socket_total = inventory.get(&socket).copied().unwrap_or(0);
        let mate_total = inventory.get(&mate).copied().unwrap_or(0);
        if socket_total > mate_total {
            constraints.insert(ClosureConstraint { socket });
        } else if mate_total > socket_total {
            constraints.insert(ClosureConstraint { socket: mate });
        }

        if policy == SocketClosurePolicy::OtherSelectedRoom {
            for &room_index in selected {
                let own_sockets = rooms[room_index].socket_count(socket);
                let own_mates = rooms[room_index].socket_count(mate);
                if own_sockets > mate_total.saturating_sub(own_mates) {
                    constraints.insert(ClosureConstraint { socket });
                }
                if own_mates > socket_total.saturating_sub(own_sockets) {
                    constraints.insert(ClosureConstraint { socket: mate });
                }
            }
        }
    }
    constraints.into_iter().collect()
}

fn pairing_components(
    rooms: &[IndexedRoom],
    selected: &[usize],
    policy: SocketClosurePolicy,
) -> Option<Vec<Vec<usize>>> {
    if !closure_constraints(rooms, selected, policy).is_empty() {
        return None;
    }

    let selected_set = selected.iter().copied().collect::<BTreeSet<_>>();
    let signatures = selected
        .iter()
        .flat_map(|&room_index| rooms[room_index].socket_counts.keys().copied())
        .map(|socket| socket.min(socket.mate()))
        .collect::<BTreeSet<_>>();
    let mut union_find = UnionFind::new(rooms.len());

    for socket in signatures {
        let mate = socket.mate();
        let mut left_rooms = Vec::new();
        let mut right_rooms = Vec::new();
        for &room_index in selected {
            left_rooms.extend(std::iter::repeat_n(
                room_index,
                rooms[room_index].socket_count(socket),
            ));
            right_rooms.extend(std::iter::repeat_n(
                room_index,
                rooms[room_index].socket_count(mate),
            ));
        }
        if left_rooms.len() != right_rooms.len() {
            return None;
        }

        let mut matched_left = vec![None; right_rooms.len()];
        for left_index in 0..left_rooms.len() {
            let mut seen_right = vec![false; right_rooms.len()];
            if !augment_socket_pairing(
                left_index,
                &left_rooms,
                &right_rooms,
                policy,
                &mut seen_right,
                &mut matched_left,
            ) {
                return None;
            }
        }
        for (right_index, matched) in matched_left.into_iter().enumerate() {
            let left_index = matched?;
            debug_assert!(socket.matches(mate));
            union_find.union(left_rooms[left_index], right_rooms[right_index]);
        }
    }

    let mut components = BTreeMap::<usize, Vec<usize>>::new();
    for room_index in selected_set {
        let root = union_find.find(room_index);
        components.entry(root).or_default().push(room_index);
    }
    let mut components = components.into_values().collect::<Vec<_>>();
    for component in &mut components {
        component.sort_unstable();
        if !closure_constraints(rooms, component, policy).is_empty() {
            return None;
        }
    }
    components.sort_unstable();
    Some(components)
}

fn augment_socket_pairing(
    left_index: usize,
    left_rooms: &[usize],
    right_rooms: &[usize],
    policy: SocketClosurePolicy,
    seen_right: &mut [bool],
    matched_left: &mut [Option<usize>],
) -> bool {
    for right_index in 0..right_rooms.len() {
        if seen_right[right_index]
            || (policy == SocketClosurePolicy::OtherSelectedRoom
                && left_rooms[left_index] == right_rooms[right_index])
        {
            continue;
        }
        seen_right[right_index] = true;
        let displaced = matched_left[right_index];
        if displaced.is_none()
            || augment_socket_pairing(
                displaced.expect("checked as present"),
                left_rooms,
                right_rooms,
                policy,
                seen_right,
                matched_left,
            )
        {
            matched_left[right_index] = Some(left_index);
            return true;
        }
    }
    false
}

struct UnionFind {
    parents: Vec<usize>,
}

impl UnionFind {
    fn new(len: usize) -> Self {
        Self {
            parents: (0..len).collect(),
        }
    }

    fn find(&mut self, index: usize) -> usize {
        let parent = self.parents[index];
        if parent != index {
            self.parents[index] = self.find(parent);
        }
        self.parents[index]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left = self.find(left);
        let right = self.find(right);
        if left == right {
            return;
        }
        let (root, child) = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        self.parents[child] = root;
    }
}

#[cfg(test)]
mod tests {
    use downwards_core::{BoundarySide, DoorSocket};

    use super::{
        SocketClosurePolicy, SocketPackageError, SocketPackageRequest, SocketPackageRoom,
        audit_socket_mate_coverage, build_socket_packages, is_pairwise_mate_closed,
    };

    fn socket(side: BoundarySide, offset: i32) -> DoorSocket {
        DoorSocket {
            side,
            offset,
            span: 20,
        }
    }

    fn request(minimum: usize, maximum: usize) -> SocketPackageRequest {
        SocketPackageRequest {
            target_min_rooms: minimum,
            target_max_rooms: maximum,
            node_budget: 10_000,
            closure_policy: SocketClosurePolicy::OtherSelectedRoom,
        }
    }

    #[test]
    fn unmatched_multiplicity_and_same_room_mates_are_rejected() {
        let left = socket(BoundarySide::Left, 40);
        let right = left.mate();
        assert!(!is_pairwise_mate_closed(
            [&[left, left][..], &[right][..]],
            SocketClosurePolicy::OtherSelectedRoom,
        ));
        assert!(!is_pairwise_mate_closed(
            [&[left, right][..]],
            SocketClosurePolicy::OtherSelectedRoom,
        ));
        assert!(is_pairwise_mate_closed(
            [&[left, right][..]],
            SocketClosurePolicy::AllowSameRoom,
        ));
    }

    #[test]
    fn reusable_corpus_coverage_requires_a_mate_on_another_room() {
        let left = [socket(BoundarySide::Left, 40)];
        let right = [left[0].mate()];
        let both = [left[0], right[0]];
        let ids = ["self-only", "left", "right"];
        let self_only = [SocketPackageRoom {
            stable_id: &ids[0],
            sockets: &both,
            eligible: true,
        }];
        let self_report = audit_socket_mate_coverage(&self_only).unwrap();
        assert!(!self_report.is_complete());
        assert_eq!(self_report.uncovered.len(), 2);

        let separate = [
            SocketPackageRoom {
                stable_id: &ids[1],
                sockets: &left,
                eligible: true,
            },
            SocketPackageRoom {
                stable_id: &ids[2],
                sockets: &right,
                eligible: true,
            },
        ];
        let report = audit_socket_mate_coverage(&separate).unwrap();
        assert!(report.is_complete());
        assert_eq!(report.covered_socket_occurrences, 2);
    }

    #[test]
    fn deterministic_choice_uses_stable_id_not_input_order() {
        let left = [socket(BoundarySide::Left, 20)];
        let right = [left[0].mate()];
        let a = "a";
        let b = "b";
        let c = "c";
        let ordered = [
            SocketPackageRoom {
                stable_id: &a,
                sockets: &left,
                eligible: true,
            },
            SocketPackageRoom {
                stable_id: &b,
                sockets: &right,
                eligible: true,
            },
            SocketPackageRoom {
                stable_id: &c,
                sockets: &right,
                eligible: true,
            },
        ];
        let shuffled = [ordered[2], ordered[0], ordered[1]];

        let expected = build_socket_packages(&ordered, request(2, 2)).unwrap();
        let actual = build_socket_packages(&shuffled, request(2, 2)).unwrap();
        assert_eq!(actual.packages, expected.packages);
        assert_eq!(actual.packages[0].room_ids, ["a", "b"]);
    }

    #[test]
    fn impossible_inventory_reports_exhausted() {
        let left = [socket(BoundarySide::Left, 20)];
        let ceiling = [socket(BoundarySide::Ceiling, 80)];
        let a = "a";
        let b = "b";
        let rooms = [
            SocketPackageRoom {
                stable_id: &a,
                sockets: &left,
                eligible: true,
            },
            SocketPackageRoom {
                stable_id: &b,
                sockets: &ceiling,
                eligible: true,
            },
        ];

        assert!(matches!(
            build_socket_packages(&rooms, request(2, 2)),
            Err(SocketPackageError::Exhausted { .. })
        ));
    }

    #[test]
    fn packages_are_independently_closed_and_range_is_not_exact() {
        let horizontal = socket(BoundarySide::Left, 20);
        let vertical = socket(BoundarySide::Ceiling, 60);
        let horizontal_left = [horizontal];
        let horizontal_right = [horizontal.mate()];
        let vertical_top = [vertical, vertical];
        let vertical_bottom = [vertical.mate(), vertical.mate()];
        let ids = ["a", "b", "c", "d"];
        let sockets = [
            &horizontal_left[..],
            &horizontal_right[..],
            &vertical_top[..],
            &vertical_bottom[..],
        ];
        let rooms = (0..ids.len())
            .map(|index| SocketPackageRoom {
                stable_id: &ids[index],
                sockets: sockets[index],
                eligible: true,
            })
            .collect::<Vec<_>>();

        let plan = build_socket_packages(&rooms, request(3, 4)).unwrap();
        assert_eq!(plan.total_rooms, 4);
        assert_eq!(
            plan.packages
                .iter()
                .map(|package| package.room_ids.as_slice())
                .collect::<Vec<_>>(),
            vec![&["a", "b"][..], &["c", "d"][..]]
        );
        for package in &plan.packages {
            let socket_multisets = package
                .room_ids
                .iter()
                .map(|id| sockets[ids.iter().position(|candidate| candidate == id).unwrap()])
                .collect::<Vec<_>>();
            assert!(is_pairwise_mate_closed(
                socket_multisets,
                SocketClosurePolicy::OtherSelectedRoom,
            ));
        }
    }

    #[test]
    fn zero_budget_is_explicitly_inconclusive() {
        let id = "socketless";
        let rooms = [SocketPackageRoom {
            stable_id: &id,
            sockets: &[],
            eligible: true,
        }];
        let mut bounded = request(1, 1);
        bounded.node_budget = 0;
        assert!(matches!(
            build_socket_packages(&rooms, bounded),
            Err(SocketPackageError::Inconclusive {
                explored_nodes: 0,
                ..
            })
        ));
    }
}
