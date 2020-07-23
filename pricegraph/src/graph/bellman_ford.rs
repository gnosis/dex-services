//! The module contains a slightly modified version of the `petgraph`
//! implementation of the Bellman-Ford graph search algorigthm that returns the
//! detected negative cycle on error.

use super::path::find_cycle2;
use super::IntegerNodeIndex;
use petgraph::algo::FloatMeasure;
use petgraph::visit::{EdgeRef, IntoEdges, IntoNodeIdentifiers, NodeCount, NodeIndexable};

/// A type definition for the result of a Bellman-Ford shortest path search
/// containing the weight of the shortest paths and the predecessor vector for
/// the shortests paths.
pub struct Path(Vec<IntegerNodeIndex>);
impl From<Cycle> for Path {
    fn from(cycle: Cycle) -> Self {
        Path(cycle.0)
    }
}

#[derive(Debug)]
pub struct Cycle(pub Vec<IntegerNodeIndex>);

impl Cycle {
    fn position(&self, target: IntegerNodeIndex) -> Option<usize> {
        self.0.iter().position(|node| *node == target)
    }
    pub fn change_starting_node(mut self, start: IntegerNodeIndex) -> Result<Cycle, Cycle> {
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
    pub fn as_path(self) -> Path {
        return Path(self.0);
    }
}

/// A negative cycle error storing the detected cycle
#[derive(Debug)]
pub struct NegativeCycle2(pub Cycle);

type Distance<W> = Vec<W>;

struct UpdatableDistance<W> {
    current: Distance<W>,
    update: Distance<W>,
}

enum PredecessorStore<W> {
    Unbounded(Vec<Option<IntegerNodeIndex>>, Distance<W>),
    Bounded(Vec<Vec<Option<IntegerNodeIndex>>>, UpdatableDistance<W>),
}
impl<W> PredecessorStore<W> {
    fn distance(&self, node_index: usize) -> &W {
        match self {
            PredecessorStore::Unbounded(_, distance) => &distance[node_index],
            PredecessorStore::Bounded(_, distance) => &distance.current[node_index],
        }
    }
    fn update_distance(&mut self, node_index: usize, updated_distance: W) {
        match self {
            PredecessorStore::Unbounded(_, distance) => {
                distance[node_index] = updated_distance;
            }
            PredecessorStore::Bounded(_, distance) => {
                distance.update[node_index] = updated_distance;
            }
        };
    }
    fn update_predecessor(
        &mut self,
        node_index: usize,
        updated_predecessor: Option<IntegerNodeIndex>,
    ) {
        match self {
            PredecessorStore::Unbounded(predecessor, _) => {
                // in the unbounded case the current vector can be updated directly
                predecessor[node_index] = updated_predecessor;
            }
            PredecessorStore::Bounded(predecessors, _) => {
                predecessors
                    .last_mut()
                    .expect("Cannot update uninitialized predecessor vector")[node_index] =
                    updated_predecessor;
            }
        };
    }
}

pub struct ShortestPathGraph<W> {
    predecessor: PredecessorStore<W>,
    source: IntegerNodeIndex,
}
impl<W> ShortestPathGraph<W> {
    fn distance(&self, node: IntegerNodeIndex) -> &W {
        self.predecessor.distance(node)
    }
    fn update_distance(&mut self, node: IntegerNodeIndex, updated_distance: W) {
        self.predecessor.update_distance(node, updated_distance);
    }
    fn update_predecessor(
        &mut self,
        node: IntegerNodeIndex,
        updated_predecessor: Option<IntegerNodeIndex>,
    ) {
        self.predecessor
            .update_predecessor(node, updated_predecessor);
    }
    pub fn find_path_to(&self, dest: IntegerNodeIndex) -> Option<Vec<IntegerNodeIndex>> {
        let mut path;
        let mut current = dest;
        match &self.predecessor {
            PredecessorStore::Unbounded(predecessor, _) => {
                path = Vec::with_capacity(predecessor.len());
                while current != self.source {
                    path.push(current);
                    current = predecessor[current]?;
                }
                path.push(self.source);

                // NOTE: `path` is in reverse order, since it was built by walking the path
                // backwards, so reverse it and done!
                path.reverse();
                Some(path)
            }
            PredecessorStore::Bounded(predecessors, _) => {
                let hops = predecessors.len();
                path = Vec::with_capacity(hops);
                for h in (0..hops).rev() {
                    current = if let Some(pred) = predecessors[h][current] {
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
    pub fn connected_nodes(&self) -> impl Iterator<Item = IntegerNodeIndex> {
        let mut node_indices: Vec<_> = match &self.predecessor {
            PredecessorStore::Unbounded(predecessor, _) => predecessor
                .iter()
                .enumerate()
                .filter_map(|(i, &pre)| pre.map(|_| i))
                .collect(),
            PredecessorStore::Bounded(predecessors, _) => predecessors
                .iter()
                .flatten()
                .enumerate()
                .filter_map(|(i, &pre)| pre.map(|_| i))
                .collect(),
        };
        node_indices.sort();
        node_indices.dedup();
        node_indices.into_iter()
    }
}
impl<G> ShortestPathGraph<G::EdgeWeight>
where
    G: IntoNodeIdentifiers + IntoEdges + NodeIndexable,
    G::EdgeWeight: FloatMeasure,
{
    fn init(graph: &G, source: G::NodeId, hops: Option<usize>) -> ShortestPathGraph<G::EdgeWeight> {
        let predecessor = vec![None; graph.node_bound()];
        let mut distance_vec = vec![<_>::infinite(); graph.node_bound()];
        distance_vec[graph.to_index(source)] = <_>::zero();

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
            source: graph.to_index(source),
        };
    }
    fn prepare_next_relaxation_step(&mut self, graph: &G) {
        if let PredecessorStore::Bounded(predecessors, distance) = &mut self.predecessor {
            predecessors.push(vec![None; graph.node_bound()]);
            distance.current = distance.update.clone();
        }
        // nothing needs to be done for the unbounded case, since the same
        // predecessor and distance vectors are reused
    }
    // check for negative weight cycle and create loop in predecessor graph
    fn mark_cycle(&mut self, graph: &G) -> Option<IntegerNodeIndex> {
        match &mut self.predecessor {
            PredecessorStore::Unbounded(predecessor, distance) => {
                for i in graph.node_identifiers() {
                    for edge in graph.edges(i) {
                        let j = edge.target();
                        let w = *edge.weight();
                        if distance[graph.to_index(i)] + w < distance[graph.to_index(j)] {
                            predecessor[graph.to_index(j)] = Some(graph.to_index(i));
                            return Some(graph.to_index(i));
                        }
                    }
                }
                return None;
            }
            PredecessorStore::Bounded(predecessors, _) => {
                let hops = predecessors.len();
                for i in graph.node_identifiers().map(|i| graph.to_index(i)) {
                    let mut node = i;
                    for h in (0..hops).rev() {
                        node = if let Some(pred) = predecessors[h][node] {
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
    fn find_cycle(&mut self, graph: &G) -> Option<Vec<IntegerNodeIndex>> {
        let search_start = match self.mark_cycle(graph) {
            Some(node) => node,
            None => return None,
        };
        match &self.predecessor {
            PredecessorStore::Unbounded(predecessor, _) => {
                find_cycle2(graph, predecessor, search_start, None)
            }
            PredecessorStore::Bounded(predecessors, _) => {
                let hops = predecessors.len();
                let mut cycle = Vec::with_capacity(hops);
                for i in graph.node_identifiers().map(|i| graph.to_index(i)) {
                    let mut node = i;
                    for h in (0..hops).rev() {
                        cycle.push(node);
                        node = if let Some(pred) = predecessors[h][node] {
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
) -> Result<ShortestPathGraph<G::EdgeWeight>, NegativeCycle2>
where
    G: NodeCount + IntoNodeIdentifiers + IntoEdges + NodeIndexable,
    G::EdgeWeight: FloatMeasure,
{
    let mut shortest_path_graph = ShortestPathGraph::init(&g, source, hops);

    // scan up to |V| - 1 times.
    for _ in 1..g.node_count() {
        let mut did_update = false;
        for i in g.node_identifiers() {
            for edge in g.edges(i) {
                let i = g.to_index(edge.source());
                let j = g.to_index(edge.target());
                let w = *edge.weight();
                if *shortest_path_graph.distance(i) + w < *shortest_path_graph.distance(j) {
                    shortest_path_graph.update_distance(j, *shortest_path_graph.distance(i) + w);
                    shortest_path_graph.update_predecessor(j, Some(i));
                    did_update = true;
                }
            }
        }
        if !did_update {
            break;
        }
    }

    let cycle = shortest_path_graph.find_cycle(&g);
    match cycle {
        Some(cycle) => Err(NegativeCycle2(Cycle(cycle))),
        None => Ok(shortest_path_graph),
    }
}
