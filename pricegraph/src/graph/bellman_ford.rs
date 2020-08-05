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

/// Stores the information needed to manage the predecessor list.
/// For each node index, this type contains its predecessor node and
/// distance in the graph from a source node.
struct PredecessorStore<G: GraphBase + Data>(Vec<Option<G::NodeId>>, Distance<G>);

impl<G: GraphBase + Data> PredecessorStore<G> {
    fn distance(&self, node_index: usize) -> &G::EdgeWeight {
        let PredecessorStore(_, distance) = self;
        &distance[node_index]
    }
    fn update_distance(&mut self, node_index: usize, updated_distance: G::EdgeWeight) {
        let PredecessorStore(_, distance) = self;
        distance[node_index] = updated_distance;
    }
    fn update_predecessor(&mut self, node_index: usize, updated_predecessor: Option<G::NodeId>) {
        let PredecessorStore(predecessor, _) = self;
        predecessor[node_index] = updated_predecessor;
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
        let PredecessorStore(predecessor, _) = &self.predecessor_store;
        path = Vec::with_capacity(predecessor.len());
        while current != self.source {
            path.push(current);
            current = predecessor[self.graph.to_index(current)]?;
        }
        path.push(self.source);

        // NOTE: `path` is in reverse order, since it was built by walking the path
        // backwards, so reverse it and done!
        path.reverse();
        Some(Path(path))
    }

    /// Lists all nodes that can be reached from the source.
    pub fn connected_nodes(&self) -> Vec<G::NodeId> {
        let PredecessorStore(predecessor, _) = &self.predecessor_store;
        let mut node_indices: Vec<_> = predecessor
            .iter()
            .enumerate()
            .filter_map(|(i, &pre)| pre.map(|_| i))
            .collect();
        // if the source was not connected to anything, no node would have it as its
        // predecessor
        node_indices.push(self.graph.to_index(self.source));
        node_indices.sort();
        node_indices.dedup();
        node_indices
            .into_iter()
            .map(|i| self.graph.from_index(i))
            .collect()
    }
}

impl<G> ShortestPathGraph<G>
where
    G: IntoNodeIdentifiers + IntoEdges + NodeIndexable,
    G::EdgeWeight: FloatMeasure,
{
    /// Initializes a shortest path graph that will be later built with the
    /// Bellman-Ford algorithm.
    fn init(g: G, source: G::NodeId) -> ShortestPathGraph<G> {
        let predecessor = vec![None; g.node_bound()];
        let mut distance_vec = vec![<_>::infinite(); g.node_bound()];
        distance_vec[g.to_index(source)] = <_>::zero();

        let predecessor_store = PredecessorStore(predecessor, distance_vec);

        ShortestPathGraph {
            graph: g,
            predecessor_store,
            source,
        }
    }

    /// Checks for negative weight cycle and, if any is found, creates loop in
    /// the predecessor store
    fn mark_cycle(&mut self) -> Option<G::NodeId> {
        // This is the last step of the Bellman-Ford algorithm. It tries to relax
        // each node: if a node can be relaxed then a negative cycle exists and is
        // created in the predecessor store; otherwise no such cycle exists.

        let PredecessorStore(predecessor, distance) = &mut self.predecessor_store;

        for i in self.graph.node_identifiers() {
            for edge in self.graph.edges(i) {
                let j = edge.target();
                let w = *edge.weight();
                if distance[self.graph.to_index(i)] + w < distance[self.graph.to_index(j)] {
                    predecessor[self.graph.to_index(j)] = Some(i);
                    return Some(j);
                }
            }
        }
        None
    }

    /// Returns a negative cycle, if it exists.
    fn find_cycle(&mut self) -> Option<NegativeCycle<G::NodeId>> {
        let search_start = match self.mark_cycle() {
            Some(node) => node,
            None => return None,
        };

        let PredecessorStore(predecessor, _) = &mut self.predecessor_store;

        find_cycle(self.graph, &predecessor, search_start, None)
    }
}

/// This implementation follows closely the one that can be found in the
/// `petgraph` crate, but it has customized output types.
///
/// The orginal source can be found here:
/// https://docs.rs/petgraph/0.5.0/src/petgraph/algo/mod.rs.html#745-792
pub fn search<G>(g: G, source: G::NodeId) -> Result<ShortestPathGraph<G>, NegativeCycle<G::NodeId>>
where
    G: NodeCount + IntoNodeIdentifiers + IntoEdges + NodeIndexable,
    G::EdgeWeight: FloatMeasure,
{
    let mut shortest_path_graph = ShortestPathGraph::init(g, source);

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
                    shortest_path_graph.update_predecessor(j, Some(i));
                    did_update = true;
                }
            }
        }
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

        let negative_cycle = search(&graph, 0.into()).err().unwrap();

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

        let shartest_path_graph = search(&graph, 0.into()).unwrap();
        let path = shartest_path_graph.path_to(5.into()).unwrap();

        assert_eq!(path, Path(vec![0.into(), 3.into(), 4.into(), 5.into()]));
    }
}
