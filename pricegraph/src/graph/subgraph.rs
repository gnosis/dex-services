//! Module implementing tools for iterating over disconnected subgraphs.

use super::bellman_ford::ShortestPathGraph;
use petgraph::visit::{Data, GraphBase, IntoEdges, NodeIndexable};
use std::cmp::Ord;
use std::collections::BTreeSet;

/// A struct used for iterating over disconnected subgraphs in the orderbook for
/// detecting orderbook overlaps and reducing the orderbook.
///
/// Note that this pseudo-iterator uses a `BTreeSet` to ensure that subgraphs
/// are visited in a predictable order starting the from the first node.
pub struct Subgraphs<G: GraphBase>(BTreeSet<G::NodeId>);

impl<G> Subgraphs<G>
where
    G: IntoEdges + NodeIndexable,
    G::NodeId: Ord,
{
    /// Create a new subgraphs iterator from an iterator of nodes.
    pub fn new(nodes: impl Iterator<Item = G::NodeId>) -> Self {
        Subgraphs::<G>(nodes.collect())
    }

    /// Iterate through each subgraph with the provided closure, returning the
    /// control flow `Break` value if there was an early return.
    pub fn for_each_until<T>(self, mut f: impl FnMut(G::NodeId) -> ControlFlow<G, T>) -> Option<T> {
        let Self(mut remaining_tokens) = self;
        while let Some(&token) = remaining_tokens.iter().next() {
            remaining_tokens.remove(&token);
            let shortest_path_graph = match f(token) {
                ControlFlow::Continue(shortest_path_graph) => shortest_path_graph,
                ControlFlow::Break(result) => return Some(result),
            };

            for connected in shortest_path_graph.connected_nodes() {
                remaining_tokens.remove(&connected);
            }
        }

        None
    }
}

/// An enum for representing control flow when iterating subgraphs.
pub enum ControlFlow<G: GraphBase + Data, T> {
    /// Continue the iterating through the subgraphs with the provided
    /// shortest path graph indicating which nodes are connected to the
    /// current subgraph.
    Continue(ShortestPathGraph<G>),
    /// Stop iterating through the subgraphs and return a result.
    Break(T),
}
