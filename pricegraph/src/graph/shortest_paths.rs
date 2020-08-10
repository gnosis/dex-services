//! The module contains a slightly modified version of the `petgraph`
//! implementation of the Bellman-Ford graph search algorigthm that returns the
//! detected negative cycle on error.

use super::path::{find_cycle, NegativeCycle, Path};
use petgraph::algo::FloatMeasure;
use petgraph::visit::{
    Data, EdgeRef, GraphBase, IntoEdges, IntoNodeIdentifiers, NodeCount, NodeIndexable,
};

/// A vector associating to each node index a distance from the source.
type Distance<G> = Vec<<G as Data>::EdgeWeight>;

type PredecessorVec<G> = Vec<Option<<G as GraphBase>::NodeId>>;

struct UpdatableDistance<G: Data> {
    current: Distance<G>,
    update: Distance<G>,
}

/// Stores the information needed to manage the predecessor list.
/// For each node index, this type contains its predecessor node and
/// distance in the graph from a source node.
enum PredecessorStore<G: GraphBase + Data> {
    Unbounded(PredecessorVec<G>, Distance<G>),
    Bounded(Vec<PredecessorVec<G>>, UpdatableDistance<G>),
}

impl<G: GraphBase + Data> PredecessorStore<G> {
    fn distance(&self, node_index: usize) -> &G::EdgeWeight {
        match self {
            PredecessorStore::Unbounded(_, distance) => &distance[node_index],
            PredecessorStore::Bounded(_, distance) => &distance.current[node_index],
        }
    }

    fn update_distance(&mut self, node_index: usize, updated_distance: G::EdgeWeight) {
        match self {
            PredecessorStore::Unbounded(_, distance) => {
                distance[node_index] = updated_distance;
            }
            PredecessorStore::Bounded(_, distance) => {
                distance.update[node_index] = updated_distance;
            }
        };
    }

    fn update_predecessor(&mut self, node_index: usize, updated_predecessor: Option<G::NodeId>) {
        match self {
            PredecessorStore::Unbounded(predecessors, _) => {
                predecessors[node_index] = updated_predecessor;
            }
            PredecessorStore::Bounded(predecessors_at_step, _) => {
                predecessors_at_step
                    .last_mut()
                    .expect("Cannot update uninitialized predecessor vector")[node_index] =
                    updated_predecessor;
            }
        };
    }
}

/// Structure that can be used to derive the shorthest path from a source to any
/// reachable destination in the graph.
pub struct ShortestPathGraph<G: GraphBase + Data> {
    graph: G,
    predecessor_store: PredecessorStore<G>,
    source: G::NodeId,
}

impl<G> ShortestPathGraph<G>
where
    G: IntoEdges + NodeIndexable,
    G::NodeId: Ord,
{
    /// Returns the current distance of a node from the source.
    fn distance(&self, node: G::NodeId) -> &G::EdgeWeight {
        self.predecessor_store.distance(self.graph.to_index(node))
    }

    /// Updates the distance of a node from the source.
    fn update_distance(&mut self, node: G::NodeId, updated_distance: G::EdgeWeight) {
        self.predecessor_store
            .update_distance(self.graph.to_index(node), updated_distance);
    }

    /// Updates the predecessor of a node.
    fn update_predecessor(&mut self, node: G::NodeId, updated_predecessor: Option<G::NodeId>) {
        self.predecessor_store
            .update_predecessor(self.graph.to_index(node), updated_predecessor);
    }

    /// Returns shortest path from source to destination node, if a path exists.
    pub fn path_to(&self, dest: G::NodeId) -> Option<Path<G::NodeId>> {
        let mut path;
        let mut current = dest;
        path = match &self.predecessor_store {
            PredecessorStore::Unbounded(predecessors, _) => {
                path = Vec::with_capacity(predecessors.len());
                while current != self.source {
                    path.push(current);
                    current = predecessors[self.graph.to_index(current)]?;
                }
                path
            }
            PredecessorStore::Bounded(predecessors_at_step, _) => {
                let steps = predecessors_at_step.len();
                path = Vec::with_capacity(steps);
                let mut step = steps;
                loop {
                    if step == 0 {
                        return None;
                    }
                    step -= 1;
                    if let Some(pred) = predecessors_at_step[step][self.graph.to_index(current)] {
                        path.push(current);
                        if pred == self.source {
                            break path;
                        }
                        current = pred;
                    }
                }
            }
        };

        path.push(self.source);
        // NOTE: `path` is in reverse order, since it was built by walking the path
        // backwards, so reverse it and done!
        path.reverse();
        Some(Path(path))
    }

    /// Lists all nodes that can be reached from the source.
    pub fn connected_nodes(&self) -> Vec<G::NodeId> {
        let mut node_indices: Vec<_> = match &self.predecessor_store {
            PredecessorStore::Unbounded(predecessors, _) => predecessors
                .iter()
                .enumerate()
                .filter_map(|(i, &pre)| pre.map(|_| self.graph.from_index(i)))
                .collect(),
            PredecessorStore::Bounded(predecessors_at_step, _) => {
                let mut repeating_node_indices: Vec<_> = predecessors_at_step
                    .iter()
                    .flatten()
                    .enumerate()
                    .filter_map(|(i, &pre)| pre.map(|_| self.graph.from_index(i)))
                    .collect();
                repeating_node_indices.sort();
                repeating_node_indices.dedup();
                repeating_node_indices
            }
        };

        debug_assert!(!node_indices.contains(&self.source));
        node_indices.push(self.source);
        node_indices
    }
}

impl<G> ShortestPathGraph<G>
where
    G: IntoNodeIdentifiers + IntoEdges + NodeIndexable,
    G::NodeId: Ord, 
    G::EdgeWeight: FloatMeasure,
{
    /// Initializes a shortest path graph that will be later built with the
    /// Bellman-Ford algorithm.
    fn empty(g: G, source: G::NodeId, hops: Option<usize>) -> Self {
        let predecessors = vec![None; g.node_bound()];
        let mut distance = vec![<_>::infinite(); g.node_bound()];
        distance[g.to_index(source)] = <_>::zero();

        let predecessor_store = match hops {
            None => PredecessorStore::Unbounded(predecessors, distance),
            Some(h) => {
                let mut predecessors_at_step: Vec<_> = Vec::with_capacity(h);
                predecessors_at_step.push(predecessors);
                let distance = UpdatableDistance {
                    current: distance.clone(),
                    update: distance,
                };
                PredecessorStore::Bounded(predecessors_at_step, distance)
            }
        };

        ShortestPathGraph {
            graph: g,
            predecessor_store,
            source,
        }
    }

    fn prepare_next_relaxation_step(&mut self) {
        if let PredecessorStore::Bounded(predecessors_at_step, distance) =
            &mut self.predecessor_store
        {
            predecessors_at_step.push(vec![None; self.graph.node_bound()]);
            distance.current = distance.update.clone();
        }
    }

    /// Checks for negative weight cycle and, if any is found, creates loop in
    /// the predecessor store
    fn mark_cycle(&mut self) -> Option<G::NodeId> {
        match &mut self.predecessor_store {
            PredecessorStore::Unbounded(..) => {
                // This is the last step of the Bellman-Ford algorithm. It tries to relax
                // each node: if a node can be relaxed then a negative cycle exists and is
                // created in the predecessor store; otherwise no such cycle exists.
                for i in self.graph.node_identifiers() {
                    for edge in self.graph.edges(i) {
                        let j = edge.target();
                        let w = *edge.weight();
                        if *self.distance(i) + w < *self.distance(j) {
                            self.update_predecessor(j, Some(i));
                            return Some(j);
                        }
                    }
                }
                return None;
            }
            PredecessorStore::Bounded(predecessors_at_step, _) => {
                let steps = predecessors_at_step.len();
                for end_node in self.graph.node_identifiers() {
                    let mut node = end_node;
                    for step in (0..steps).rev() {
                        node = if let Some(pred) =
                            predecessors_at_step[step][self.graph.to_index(node)]
                        {
                            if pred == end_node {
                                return Some(end_node);
                            }
                            pred
                        } else {
                            node
                        };
                    }
                }
                println!("none");
                None
            }
        }
    }

    /// Returns a negative cycle, if it exists.
    fn find_cycle(&mut self) -> Option<NegativeCycle<G::NodeId>> {
        let search_start = match self.mark_cycle() {
            Some(node) => node,
            None => return None,
        };
        match &self.predecessor_store {
            PredecessorStore::Unbounded(predecessors, _) => {
                find_cycle(self.graph, &predecessors, search_start, None)
            }
            PredecessorStore::Bounded(predecessors_at_step, _) => {
                let steps = predecessors_at_step.len();
                let mut cycle = Vec::with_capacity(steps);
                let mut node = search_start;
                for step in (0..steps).rev() {
                    node = if let Some(pred) = predecessors_at_step[step][self.graph.to_index(node)] {
                        cycle.push(node);
                        if pred == search_start {
                            cycle.push(search_start);

                            // NOTE: `cycle` is in reverse order, since it was built by walking the cycle
                            // backwards, so reverse it and done!
                            cycle.reverse();

                            return Some(NegativeCycle(cycle));
                        }
                        pred
                    } else {
                        node
                    };
                }
                panic!("Detected cycle could not be found")
            }
        }
    }
}

impl<G> ShortestPathGraph<G>
where
    G: NodeCount + IntoNodeIdentifiers + IntoEdges + NodeIndexable,
    G::NodeId: Ord,
    G::EdgeWeight: FloatMeasure,
{
    /// Creates a representation of all shortest paths from the given source
    /// to any other node in the graph.
    ///
    /// Shortest paths are well defined if and only if the graph does not
    /// contain any negative weight cycle reachabe from the source. If a
    /// negative weight cycle is detected, it is returned as an error.
    pub fn new(
        g: G,
        source: G::NodeId,
        hops: Option<usize>,
    ) -> Result<Self, NegativeCycle<G::NodeId>> {
        bellman_ford(g, source, hops)
    }
}

/// This implementation follows closely the one that can be found in the
/// `petgraph` crate, but it has customized output types.
///
/// The orginal source can be found here:
/// https://docs.rs/petgraph/0.5.0/src/petgraph/algo/mod.rs.html#745-792
fn bellman_ford<G>(
    g: G,
    source: G::NodeId,
    hops: Option<usize>,
) -> Result<ShortestPathGraph<G>, NegativeCycle<G::NodeId>>
where
    G: NodeCount + IntoNodeIdentifiers + IntoEdges + NodeIndexable,
    G::NodeId: Ord,
    G::EdgeWeight: FloatMeasure,
{
    let mut shortest_path_graph = ShortestPathGraph::empty(g, source, hops);

    // scan up to |V| - 1 times.
    for _ in 1..=hops.unwrap_or(g.node_count() - 1) {
        let mut did_update = false;
        for i in g.node_identifiers() {
            for edge in g.edges(i) {
                let i = edge.source();
                let j = edge.target();
                let w = *edge.weight();
                if *shortest_path_graph.distance(i) + w < *shortest_path_graph.distance(j) {
                    shortest_path_graph.update_distance(j, *shortest_path_graph.distance(i) + w);
                    shortest_path_graph.update_predecessor(j, Some(i));
                    did_update = true;
                }
            }
        }
        shortest_path_graph.prepare_next_relaxation_step();
        if !did_update {
            break;
        }
    }

    match shortest_path_graph.find_cycle() {
        Some(negative_cycle) => Err(negative_cycle),
        None => Ok(shortest_path_graph),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use petgraph::Graph;

    #[test]
    fn search_finds_negative_cycle() {
        // NOTE: There is a negative cycle from 1 -> 2 -> 3 -> 1 with a
        // transient weight of -1.
        let graph = Graph::<(), f64>::from_edges(&[
            (0, 1, 1.0),
            (1, 2, 2.0),
            (1, 4, -100.0),
            (2, 3, 3.0),
            (3, 1, -6.0),
            (4, 3, 200.0),
        ]);

        let negative_cycle = ShortestPathGraph::new(&graph, 0.into(), None)
            .err()
            .unwrap();

        assert_eq!(
            negative_cycle.0,
            vec![1.into(), 2.into(), 3.into(), 1.into()]
        );

        let negative_cycle = negative_cycle.with_starting_node(2.into()).unwrap();
        assert_eq!(
            negative_cycle.0,
            vec![2.into(), 3.into(), 1.into(), 2.into()]
        );

        // NOTE: if `origin` is provided, but not part of the cycle, then the
        // original cycle is returned as part of the error.
        let negative_cycle = negative_cycle.with_starting_node(4.into()).unwrap_err();
        assert_eq!(
            negative_cycle.0,
            vec![2.into(), 3.into(), 1.into(), 2.into()]
        );
    }

    #[test]
    fn search_finds_shortest_path() {
        //  0 --2.0-> 1 --1.0-> 2
        //  |         |         |
        // 4.0       7.0        |
        //  v         v         |
        //  3         5        5.0
        //  |         ^         |
        // 1.0       1.0        |
        //  |         |         |
        //  \-------> 4 <-------/
        let graph = Graph::<(), f64>::from_edges(&[
            (0, 1, 2.0),
            (0, 3, 4.0),
            (1, 2, 1.0),
            (1, 5, 7.0),
            (2, 4, 5.0),
            (4, 5, 1.0),
            (3, 4, 1.0),
        ]);

        let shortest_path_graph = ShortestPathGraph::new(&graph, 0.into(), None).unwrap();
        let path = shortest_path_graph.path_to(5.into()).unwrap();

        assert_eq!(path, Path(vec![0.into(), 3.into(), 4.into(), 5.into()]));
    }

    #[test]
    fn bounded_search_finds_shortest_path() {
        //  0 --2.0-> 1 --1.0-> 2
        //  |         |         |
        // 4.0       7.0        |
        //  v         v         |
        //  3         5        5.0
        //  |         ^         |
        // 1.0       1.0        |
        //  |         |         |
        //  \-------> 4 <-------/
        let graph = Graph::<(), f64>::from_edges(&[
            (0, 1, 2.0),
            (0, 3, 4.0),
            (1, 2, 1.0),
            (1, 5, 7.0),
            (2, 4, 5.0),
            (4, 5, 1.0),
            (3, 4, 1.0),
        ]);

        let shortest_path_graph = ShortestPathGraph::new(&graph, 0.into(), Some(0)).unwrap();
        assert_eq!(shortest_path_graph.path_to(5.into()), None);

        let shortest_path_graph = ShortestPathGraph::new(&graph, 0.into(), Some(1)).unwrap();
        assert_eq!(shortest_path_graph.path_to(5.into()), None);

        let shortest_path_graph = ShortestPathGraph::new(&graph, 0.into(), Some(2)).unwrap();
        let path = shortest_path_graph.path_to(5.into()).unwrap();
        assert_eq!(path, Path(vec![0.into(), 1.into(), 5.into()]));

        let shortest_path_graph = ShortestPathGraph::new(&graph, 0.into(), Some(3)).unwrap();
        let path = shortest_path_graph.path_to(5.into()).unwrap();
        assert_eq!(path, Path(vec![0.into(), 3.into(), 4.into(), 5.into()]));

        let shortest_path_graph = ShortestPathGraph::new(&graph, 0.into(), Some(1000)).unwrap();
        let path = shortest_path_graph.path_to(5.into()).unwrap();
        // The four-step path [0, 1, 2, 4, 5] has larger weight than [0, 3, 4, 5].
        assert_eq!(path, Path(vec![0.into(), 3.into(), 4.into(), 5.into()]));
    }

    #[test]
    fn bounded_search_with_negative_cycle_through_origin() {
        //    0 --1.0-> 1
        //    ∧         |
        // -100.0      1.0
        //    |         v
        //    3 <-1.0-- 2
        let graph =
            Graph::<(), f64>::from_edges(&[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (3, 0, -100.0)]);
        let shortest_path_graph = ShortestPathGraph::new(&graph, 0.into(), Some(3)).unwrap();
        let path = shortest_path_graph.path_to(3.into()).unwrap();
        assert_eq!(path, Path(vec![0.into(), 1.into(), 2.into(), 3.into()]));

        let negative_cycle = ShortestPathGraph::new(&graph, 0.into(), Some(4))
            .err()
            .unwrap();
        assert_eq!(
            negative_cycle.0,
            vec![0.into(), 1.into(), 2.into(), 3.into(), 0.into()]
        );
    }

    #[test]
    fn bounded_search_with_negative_cycle_not_involving_origin() {
        //   0 --1.0-> 1 --1.0-> 2
        //             ∧         |
        //          -100.0      1.0
        //             |         v
        //             4 <-1.0-- 3
        let graph = Graph::<(), f64>::from_edges(&[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (3, 4, 1.0),
            (4, 1, -100.0),
        ]);
        let shortest_path_graph = ShortestPathGraph::new(&graph, 0.into(), Some(4)).unwrap();
        let path = shortest_path_graph.path_to(4.into()).unwrap();
        assert_eq!(
            path,
            Path(vec![0.into(), 1.into(), 2.into(), 3.into(), 4.into()])
        );

        let negative_cycle = ShortestPathGraph::new(&graph, 0.into(), Some(5))
            .err()
            .unwrap();
        assert_eq!(
            negative_cycle.0,
            vec![1.into(), 2.into(), 3.into(), 4.into(), 1.into()]
        );
    }

    #[test]
    fn shortest_path_graph_finds_connected_nodes() {
        //           2 <-1.0-- 3
        //           ∧
        //          1.0
        //           |
        // 1 --1.0-> 0 <-1.0-- 7 <---- -4.0
        //           |         ∧         |
        //          1.0       1.0        |
        //           v         |         |
        //           4 --1.0-> 5 --1.0-> 6
        let graph = Graph::<(), f64>::from_edges(&[
            (1, 0, 1.0),
            (0, 2, 1.0),
            (3, 2, 1.0),
            (0, 4, 1.0),
            (4, 5, 1.0),
            (5, 6, 1.0),
            (5, 7, 1.0),
            (6, 7, -4.0),
            (7, 0, 1.0),
        ]);

        let shortest_path_graph = ShortestPathGraph::new(&graph, 0.into(), None).unwrap();
        let mut connected_nodes = shortest_path_graph.connected_nodes();
        connected_nodes.sort();

        assert_eq!(
            connected_nodes,
            vec![0.into(), 2.into(), 4.into(), 5.into(), 6.into(), 7.into()]
        );
    }
}
