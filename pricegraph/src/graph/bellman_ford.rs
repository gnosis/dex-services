//! The module contains a slightly modified version of the `petgraph`
//! implementation of the Bellman-Ford graph search algorigthm that returns the
//! detected negative cycle on error.

use petgraph::algo::FloatMeasure;
use petgraph::visit::{
    Data, EdgeRef, GraphBase, IntoEdges, IntoNodeIdentifiers, NodeCount, NodeIndexable,
};

/// A type definition for the result of a Bellman-Ford shortest path search
/// containing the weight of the shortest paths and the predecessor vector for
/// the shortests paths.
pub type Paths<G> = (
    Vec<<G as Data>::EdgeWeight>,
    Vec<Option<<G as GraphBase>::NodeId>>,
);

/// A negative cycle error with a path representing the cycle.
pub struct NegativeCycle<G: Data>(pub Vec<G::NodeId>);

/// This implementation is taken from the `petgraph` crate with a small
/// modification to return the path when a negative cycle is detected.
///
/// The orginal source can be found here:
/// https://docs.rs/petgraph/0.5.0/src/petgraph/algo/mod.rs.html#745-792
pub fn search<G>(g: G, source: G::NodeId) -> Result<Paths<G>, NegativeCycle<G>>
where
    G: NodeCount + IntoNodeIdentifiers + IntoEdges + NodeIndexable,
    G::EdgeWeight: FloatMeasure,
{
    let mut predecessor = vec![None; g.node_bound()];
    let mut distance = vec![<_>::infinite(); g.node_bound()];

    let ix = |i| g.to_index(i);

    distance[ix(source)] = <_>::zero();
    // scan up to |V| - 1 times.
    for _ in 1..g.node_count() {
        let mut did_update = false;
        for i in g.node_identifiers() {
            for edge in g.edges(i) {
                let i = edge.source();
                let j = edge.target();
                let w = *edge.weight();
                if distance[ix(i)] + w < distance[ix(j)] {
                    distance[ix(j)] = distance[ix(i)] + w;
                    predecessor[ix(j)] = Some(i);
                    did_update = true;
                }
            }
        }
        if !did_update {
            break;
        }
    }

    // check for negative weight cycle
    for i in g.node_identifiers() {
        for edge in g.edges(i) {
            let j = edge.target();
            let w = *edge.weight();
            if distance[ix(i)] + w < distance[ix(j)] {
                // NOTE: The following 4 lines are the only modifications made
                // to the original algorithm and were originally:
                // ```diff
                // -//println!("neg cycle, detected from {} to {}, weight={}", i, j, w);
                // -return Err(NegativeCycle(()));
                // ```
                predecessor[ix(j)] = Some(i);
                let cycle = find_cycle::<G>(g, j, &predecessor)
                    .expect("negative cycle not found after being detected");
                return Err(NegativeCycle(cycle));
            }
        }
    }

    Ok((distance, predecessor))
}

/// Finds a cycle and returns a vector representing a path along the cycle,
/// ending that is the predecessor of the starting node.
///
/// Returns `None` if no such cycle can be found.
fn find_cycle<G>(
    g: G,
    start: G::NodeId,
    predecessor: &[Option<G::NodeId>],
) -> Option<Vec<G::NodeId>>
where
    G: NodeCount + NodeIndexable,
{
    let ix = |i| g.to_index(i);

    // NOTE: First find a node that is actually on the cycle, this is done
    // because a negative cycle can be detected on any node connected to the
    // cycle and not just nodes on the cycle itself.
    let mut visited = vec![false; g.node_bound()];
    let mut current = start;
    visited[ix(current)] = true;
    loop {
        current = predecessor[ix(current)]?;
        if visited[ix(current)] {
            break;
        }
        visited[ix(current)] = true;
    }

    // NOTE: `current` is now guaranteed to be on the cycle, so just walk
    // backwards until we reach `current` again.
    let start = current;
    let mut path = Vec::with_capacity(g.node_bound());
    loop {
        current = predecessor[ix(current)]?;
        path.push(current);
        if current == start {
            break;
        }
    }

    // NOTE: `path` is in reverse order, since it was built by walking the cycle
    // backwards, so reverse it and done!
    path.reverse();
    Some(path)
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

        let NegativeCycle(path) = search(&graph, 0.into()).unwrap_err();

        assert_eq!(path, &[1.into(), 2.into(), 3.into()]);
    }
}
