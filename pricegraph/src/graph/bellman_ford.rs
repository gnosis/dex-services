//! The module contains a slightly modified version of the `petgraph`
//! implementation of the Bellman-Ford graph search algorigthm that returns the
//! detected negative cycle on error.

use petgraph::algo::FloatMeasure;
use petgraph::visit::{EdgeRef, IntoEdges, IntoNodeIdentifiers, NodeCount, NodeIndexable};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// A type definition for the result of a Bellman-Ford shortest path search
/// containing the weight of the shortest paths and the predecessor vector for
/// the shortests paths.
pub struct Path<N>(Vec<N>);
impl<N> From<Cycle<N>> for Path<N> {
    fn from(cycle: Cycle<N>) -> Self {
        Path(cycle.0)
    }
}

#[derive(Debug)]
pub struct Cycle<N>(pub Vec<N>);

impl<N: Copy + PartialEq> Cycle<N> {
    fn position(&self, target: N) -> Option<usize> {
        self.0.iter().position(|node| *node == target)
    }
    pub fn change_starting_node(mut self, start: N) -> Result<Cycle<N>, Cycle<N>> {
        let mut cycle = Vec::with_capacity(self.0.len());
        cycle.push(start);
        let mut cycle_end = Vec::with_capacity(self.0.len());
        let mut self_iterator = self.0.into_iter();
        while let Some(node) = self_iterator.next() {
            if node == start {
                cycle.append(&mut self_iterator.collect());
                cycle.append(&mut cycle_end);
                return Ok(Cycle(cycle));
            } else {
                cycle_end.push(node);
            }
        }
        Err(Cycle(cycle_end))
    }
    pub fn as_path(self) -> Path<N> {
        return Path(self.0);
    }
}

/// A negative cycle error storing the detected cycle
#[derive(Debug)]
pub struct NegativeCycle<N>(pub Cycle<N>);

type Distance<N, W> = HashMap<N, W>;

struct UpdatableDistance<N, W> {
    current: Distance<N, W>,
    update: Distance<N, W>,
}

enum PredecessorStore<G: NodeIndexable + IntoEdges> {
    Unbounded(
        HashMap<G::NodeId, G::NodeId>,
        Distance<G::NodeId, G::EdgeWeight>,
    ),
    Bounded(
        Vec<HashMap<G::NodeId, G::NodeId>>,
        UpdatableDistance<G::NodeId, G::EdgeWeight>,
    ),
}
impl<G> PredecessorStore<G>
where
    G: NodeIndexable + IntoEdges,
    G::NodeId: Hash + Ord + Copy,
    G::EdgeWeight: FloatMeasure,
{
    fn distance(&self, node: G::NodeId) -> &G::EdgeWeight {
        match self {
            PredecessorStore::Unbounded(_, distance) => distance.get(&node),
            PredecessorStore::Bounded(_, distance) => distance.current.get(&node),
        }
        .unwrap_or(&<_>::infinite())
    }
    fn update_distance(&mut self, node: G::NodeId, updated_distance: G::EdgeWeight) {
        match self {
            PredecessorStore::Unbounded(_, distance) => {
                distance.insert(node, updated_distance);
            }
            PredecessorStore::Bounded(_, distance) => {
                distance.update.insert(node, updated_distance);
            }
        };
    }
    fn update_predecessor(&mut self, node: G::NodeId, updated_predecessor: G::NodeId) {
        match self {
            PredecessorStore::Unbounded(predecessor, _) => {
                // in the unbounded case the current vector can be updated directly
                predecessor.insert(node, updated_predecessor);
            }
            PredecessorStore::Bounded(predecessors, _) => {
                predecessors
                    .last_mut()
                    .expect("Cannot update uninitialized predecessor vector")
                    .insert(node, updated_predecessor);
            }
        };
    }
}

pub struct ShortestPathGraph<G: NodeIndexable + IntoEdges> {
    predecessor: PredecessorStore<G>,
    source: G::NodeId,
}
impl<G> ShortestPathGraph<G>
where
    G: IntoEdges + NodeIndexable,
    G::NodeId: Hash + Ord + Copy,
    G::EdgeWeight: FloatMeasure,
{
    fn distance(&self, node: G::NodeId) -> &G::EdgeWeight {
        self.predecessor.distance(node)
    }
    fn update_distance(&mut self, node: G::NodeId, updated_distance: G::EdgeWeight) {
        self.predecessor.update_distance(node, updated_distance);
    }
    fn update_predecessor(&mut self, node: G::NodeId, updated_predecessor: G::NodeId) {
        self.predecessor
            .update_predecessor(node, updated_predecessor);
    }
    pub fn find_path_to<'a>(&self, dest: G::NodeId) -> Option<Vec<G::NodeId>> {
        let mut path;
        let mut current = dest;
        match self.predecessor {
            PredecessorStore::Unbounded(predecessor, _) => {
                path = Vec::with_capacity(predecessor.len());
                while current != self.source {
                    path.push(current);
                    current = *predecessor.get(&current)?;
                }
                path.push(self.source);

                // NOTE: `path is in reverse order, since it was built by walking the path
                // backwards, so reverse it and done!
                path.reverse();
                Some(path)
            }
            PredecessorStore::Bounded(predecessors, _) => {
                let hops = predecessors.len();
                path = Vec::with_capacity(hops);
                for h in (0..hops).rev() {
                    current = if let Some(&pred) = predecessors[h].get(&current) {
                        path.push(current);
                        if pred == self.source {
                            path.push(self.source);
                            return Some(path);
                        }
                        pred
                    } else {
                        current
                    };
                }
                None
            }
        }
    }
    pub fn connected_nodes(&self) -> Vec<G::NodeId> {
        //impl Iterator<Item = G::NodeId>

        let mut nodes: Vec<_> = match self.predecessor {
            PredecessorStore::Unbounded(predecessor, _) => predecessor.keys().copied().collect(),
            PredecessorStore::Bounded(predecessors, _) => predecessors
                .iter()
                .map(|predecessor| predecessor.keys().copied().collect::<Vec<_>>())
                .flatten()
                .collect(),
        };
        nodes.sort();
        nodes.dedup();
        nodes
    }
}
impl<G> ShortestPathGraph<G>
where
    G: IntoNodeIdentifiers + IntoEdges + NodeIndexable,
    G::NodeId: Hash + Ord + Copy,
    G::EdgeWeight: FloatMeasure,
{
    fn init(g: G, source: G::NodeId, hops: Option<usize>) -> ShortestPathGraph<G> {
        let predecessor = HashMap::with_capacity(g.node_bound());
        let mut distance_vec = HashMap::with_capacity(g.node_bound());
        distance_vec.insert(source, <_>::zero());

        let predecessor_store = match hops {
            Some(h) => {
                let mut predecessors: Vec<_> = Vec::with_capacity(h);
                predecessors.push(predecessor);
                let distance = UpdatableDistance {
                    current: distance_vec.clone(),
                    update: distance_vec,
                };
                PredecessorStore::Bounded(predecessors, distance)
            }
            None => PredecessorStore::Unbounded(predecessor, distance_vec),
        };

        return ShortestPathGraph {
            predecessor: predecessor_store,
            source,
        };
    }
    fn prepare_next_relaxation_step(&mut self, node_bound: usize) {
        if let PredecessorStore::Bounded(predecessors, distance) = &mut self.predecessor {
            predecessors.push(HashMap::with_capacity(node_bound));
            distance.current = distance.update.clone();
        }
        // nothing needs to be done for the unbounded case, since the same
        // predecessor and distance vectors are reused
    }
    // check for negative weight cycle and create loop in predecessor graph
    fn mark_cycle(&mut self, graph: G) -> Option<G::NodeId> {
        match &mut self.predecessor {
            PredecessorStore::Unbounded(_, _) => {
                for i in graph.node_identifiers() {
                    for edge in graph.edges(i) {
                        let j = edge.target();
                        let w = *edge.weight();
                        if *self.predecessor.distance(i) + w < *self.predecessor.distance(j) {
                            self.predecessor.update_predecessor(j, i);
                            return Some(i);
                        }
                    }
                }
                return None;
            }
            PredecessorStore::Bounded(predecessors, _) => {
                let hops = predecessors.len();
                for i in graph.node_identifiers() {
                    let mut node = i;
                    for h in (0..hops).rev() {
                        node = if let Some(&pred) = predecessors[h].get(&node) {
                            if pred == node {
                                return Some(i);
                            }
                            pred
                        } else {
                            node
                        };
                    }
                }
                return None;
            }
        }
    }
    // detect and set up negative weight cycle if possible
    fn find_cycle(&mut self, graph: G) -> Option<Vec<G::NodeId>> {
        let search_start = match self.mark_cycle(graph) {
            Some(node) => node,
            None => return None,
        };
        match &self.predecessor {
            PredecessorStore::Unbounded(predecessor, _) => {
                // NOTE: First find a node that is actually on the cycle, this is done
                // because a negative cycle can be detected on any node connected to the
                // cycle and not just nodes on the cycle itself.
                let mut visited = HashSet::with_capacity(graph.node_bound());
                let mut cursor = search_start;
                let mut size_bound = 1;
                let cycle_start = loop {
                    cursor = *predecessor.get(&cursor)?;
                    if visited.contains(&cursor) {
                        break cursor;
                    }
                    visited.insert(cursor);
                    size_bound += 1;
                };
                let mut cycle = Vec::with_capacity(size_bound);
                loop {
                    cycle.push(cursor);
                    cursor = *predecessor
                        .get(&cursor)
                        .expect("Element not found in second traversal of the loop.");
                    if cursor == cycle_start {
                        break;
                    }
                }
                Some(cycle)
            }
            PredecessorStore::Bounded(predecessors, _) => {
                let hops = predecessors.len();
                let mut cycle = Vec::with_capacity(hops);
                for i in graph.node_identifiers() {
                    let mut node = i;
                    for h in (0..hops).rev() {
                        cycle.push(node);
                        node = if let Some(&pred) = predecessors[h].get(&node) {
                            if pred == node {
                                cycle.push(node);
                                return Some(cycle);
                            }
                            pred
                        } else {
                            node
                        };
                    }
                }
                None
            }
        }
    }
}

/// This implementation is taken from the `petgraph` crate with a small
/// modification to return the path when a negative cycle is detected.
///
/// The orginal source can be found here:
/// https://docs.rs/petgraph/0.5.0/src/petgraph/algo/mod.rs.html#745-792
pub fn search<G>(
    g: G,
    source: G::NodeId,
    hops: Option<usize>,
) -> Result<ShortestPathGraph<G>, NegativeCycle<G::NodeId>>
where
    G: NodeCount + IntoNodeIdentifiers + IntoEdges + NodeIndexable,
    G::NodeId: Hash + Ord + Copy,
    G::EdgeWeight: FloatMeasure,
{
    let mut shortest_path_graph = ShortestPathGraph::init(g, source, hops);

    // scan up to |V| - 1 times.
    for _ in 1..g.node_count() {
        let mut did_update = false;
        for i in g.node_identifiers() {
            for edge in g.edges(i) {
                let i = edge.source();
                let j = edge.target();
                let w = *edge.weight();
                if *shortest_path_graph.distance(i) + w < *shortest_path_graph.distance(j) {
                    shortest_path_graph.update_distance(j, *shortest_path_graph.distance(i) + w);
                    shortest_path_graph.update_predecessor(j, i);
                    did_update = true;
                }
            }
        }
        if !did_update {
            break;
        }
    }

    let cycle = shortest_path_graph.find_cycle(g);
    match cycle {
        Some(cycle) => Err(NegativeCycle(Cycle(cycle))),
        None => Ok(shortest_path_graph),
    }
}
