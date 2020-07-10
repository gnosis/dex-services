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

/// A negative cycle error with the node on which it was detected along with the
/// predecessor vector that can be used to re-create the cycle path.
#[derive(Debug)]
pub struct NegativeCycle<N>(pub Vec<Option<N>>, pub N);

/// This implementation is taken from the `petgraph` crate with a small
/// modification to return the path when a negative cycle is detected.
///
/// The orginal source can be found here:
/// https://docs.rs/petgraph/0.5.0/src/petgraph/algo/mod.rs.html#745-792
pub fn search<G>(g: G, source: G::NodeId) -> Result<Paths<G>, NegativeCycle<G::NodeId>>
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
                return Err(NegativeCycle(predecessor, j));
            }
        }
    }

    Ok((distance, predecessor))
}
