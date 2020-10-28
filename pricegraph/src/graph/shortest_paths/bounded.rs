use super::{Distances, PredecessorVec, PredecessorStoring, nodes_from_predecessors};
use super::super::path::{NegativeCycle, Path};
use petgraph::visit::{Data, NodeIndexable, IntoNodeIdentifiers};
use petgraph::algo::FloatMeasure;
use std::collections::HashSet;
use std::hash::Hash;
use std::marker::PhantomData;

struct UpdatableDistances<G: Data> {
    current: Distances<G>,
    pending: Distances<G>,
}

pub struct Bounded<'a, G: 'a + Data> {
    predecessors_at_step: Vec<PredecessorVec<G>>,
    distances: UpdatableDistances<G>,
    phantom: PhantomData<&'a G>
}

impl<'a, G: 'a + Data> Bounded<'a, G> 
where 
    G::EdgeWeight: FloatMeasure
{
    pub fn new(predecessors: PredecessorVec<G>, distances: Distances<G>, bound: usize) -> Self {
        let mut predecessors_at_step: Vec<_> = Vec::with_capacity(bound);
        predecessors_at_step.push(predecessors);
        let distances = UpdatableDistances {
            current: distances.clone(),
            pending: distances,
        };
        Self {
            predecessors_at_step, 
            distances,
            phantom: PhantomData,
        }
    }
}

impl<'a, G> PredecessorStoring<G> for Bounded<'a, G> 
where 
    G: 'a + Data + NodeIndexable + IntoNodeIdentifiers,
    G::NodeId: Ord + Hash,
    G::EdgeWeight: FloatMeasure
{
    fn distance(&self, node_index: usize) -> G::EdgeWeight {
        self.distances.current[node_index]
    }

    fn update_distance(&mut self, node_index: usize, updated_distance: G::EdgeWeight) {
        self.distances.pending[node_index] = updated_distance
    }

    fn update_predecessor(&mut self, node_index: usize, updated_predecessor: Option<G::NodeId>) {
        self.predecessors_at_step
                    .last_mut()
                    .expect("Cannot update uninitialized predecessor vector")[node_index] =
                    updated_predecessor;
    }

    fn path_to(&self, source: G::NodeId, dest: G::NodeId, graph: G) -> Option<Path<G::NodeId>> {
        let mut path;
        let mut current = dest;
        let max_path_len = self.predecessors_at_step.len();
        path = Vec::with_capacity(max_path_len);
        let mut found = false;
        for step in (0..max_path_len).rev() {
            if let Some(pred) = self.predecessors_at_step[step][graph.to_index(current)] {
                path.push(current);
                if pred == source {
                    found = true;
                    break;
                }
                current = pred;
            }
        }
        match found {
            false => None,
            true => {
                path.push(source);
                // NOTE: `path` is in reverse order, since it was built by walking the path
                // backwards, so reverse it and done!
                path.reverse();
                Some(Path(path))
            }
        }
    }

    fn connected_nodes(&self, graph: G) -> Vec<G::NodeId>  {
        self.predecessors_at_step
            .iter()
            .map(|predecessors| nodes_from_predecessors(graph, &predecessors))
            .flatten()
            .collect::<HashSet<_>>()
            .drain()
            .collect()
    }

    fn prepare_next_relaxation_step(&mut self) {
        // We always instantiate the Bounded store with the correct node count for step 0
        let node_count = self.predecessors_at_step[0].len();
        self.predecessors_at_step.push(vec![None; node_count]);
        self.distances.current = self.distances.pending.clone();
    }

    fn mark_cycle(&mut self, graph: G) -> Option<G::NodeId> {
        let steps = self.predecessors_at_step.len();
        for end_node in graph.node_identifiers() {
            let mut node = end_node;
            for step in (0..steps).rev() {
                node = if let Some(pred) =
                    self.predecessors_at_step[step][graph.to_index(node)]
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
        None
    }

    fn find_cycle(&mut self, search_start: G::NodeId, graph: G) -> Option<NegativeCycle<G::NodeId>> {
        let steps = self.predecessors_at_step.len();
        let mut cycle = Vec::with_capacity(steps);
        let mut node = search_start;
        for step in (0..steps).rev() {
            node = if let Some(pred) = self.predecessors_at_step[step][graph.to_index(node)]
            {
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