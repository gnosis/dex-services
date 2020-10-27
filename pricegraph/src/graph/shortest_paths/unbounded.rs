use super::{Distances, PredecessorVec, PredecessorStoring, nodes_from_predecessors};
use super::super::path::{find_cycle, NegativeCycle, Path};
use petgraph::visit::{Data, NodeIndexable, IntoNodeIdentifiers, IntoEdges, EdgeRef};
use petgraph::algo::FloatMeasure;

pub struct Unbounded<G: Data> {
    predecessors: PredecessorVec<G>, 
    distances: Distances<G>,
}

impl<G: Data> Unbounded<G> {
    pub fn new(predecessors: PredecessorVec<G>, distances: Distances<G>) -> Self {
        Self {
            predecessors,
            distances,
        }
    }
}

impl<G> PredecessorStoring<G> for Unbounded<G> 
where
    G: Data + NodeIndexable + IntoNodeIdentifiers + IntoEdges,
    G::EdgeWeight: FloatMeasure,
{
    fn distance(&self, node_index: usize) -> G::EdgeWeight {
        self.distances[node_index]
    }

    fn update_distance(&mut self, node_index: usize, update_distance: G::EdgeWeight) {
        self.distances[node_index] = update_distance
    }

    fn update_predecessor(&mut self, node_index: usize, updated_predecessor: Option<G::NodeId>) {
        self.predecessors[node_index] = updated_predecessor;
    }

    fn path_to(&self, source: G::NodeId, dest: G::NodeId, graph: G) -> Option<Path<G::NodeId>> {        let max_path_len = self.predecessors.len();
        let mut path = Vec::with_capacity(max_path_len);
        let mut current = dest;
        while current != source {
            assert!(path.len() <= max_path_len, "undetected negative cycle");
            path.push(current);
            current = self.predecessors[graph.to_index(current)]?;
        }
        path.push(source);
        // NOTE: `path` is in reverse order, since it was built by walking the path
        // backwards, so reverse it and done!
        path.reverse();
        Some(Path(path))
    }

    fn connected_nodes(&self, graph: G) -> Vec<G::NodeId> {
        nodes_from_predecessors(graph, &self.predecessors)
    }

    fn prepare_next_relaxation_step(&mut self) {}

    fn mark_cycle(&mut self, graph: G) -> Option<G::NodeId> {
        for i in graph.node_identifiers() {
            for edge in graph.edges(i) {
                let j = edge.target();
                let w = *edge.weight();
                if self.distance(graph.to_index(i)) + w < self.distance(graph.to_index(j)) {
                    self.update_predecessor(graph.to_index(j), Some(i));
                    return Some(j);
                }
            }
        }
        None
    }

    fn find_cycle(&mut self, search_start: G::NodeId, graph: G) -> Option<NegativeCycle<G::NodeId>> {
        find_cycle(graph, &self.predecessors, search_start, None)
    }
}