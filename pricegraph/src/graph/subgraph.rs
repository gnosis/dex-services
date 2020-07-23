//! Module implementing tools for iterating over disconnected subgraphs.

use super::bellman_ford::ShortestPathGraph;
use super::IntegerNodeIndex;
use petgraph::algo::FloatMeasure;
use petgraph::visit::{EdgeRef, IntoEdges, IntoNodeIdentifiers, NodeCount, NodeIndexable};
use std::collections::BTreeSet;

/// A struct used for iterating over disconnected subgraphs in the orderbook for
/// detecting orderbook overlaps and reducing the orderbook.
///
/// Note that this pseudo-iterator uses a `BTreeSet` to ensure that subgraphs
/// are visited in a predictable order starting the from the first node.
pub struct Subgraphs(BTreeSet<IntegerNodeIndex>);

impl<G> Subgraphs
where
    G: IntoEdges + NodeIndexable,
    G::EdgeWeight: FloatMeasure,
{
    /// Create a new subgraphs iterator from an iterator of node indices.
    pub fn new(graph: &G, nodes: impl Iterator<Item = IntegerNodeIndex>) -> Self {
        Subgraphs(nodes.collect())
    }

    /// Iterate through each subgraph with the provided closure returning the
    /// predecessor vector for the current node indicating which nodes are
    /// connected to it.
    pub fn for_each(
        self,
        graph: &G,
        mut f: impl FnMut(IntegerNodeIndex) -> ShortestPathGraph<G::EdgeWeight>,
    ) {
        self.for_each_until(graph, |node| {
            <ControlFlow<G::EdgeWeight, ()>>::Continue(f(node))
        });
    }

    /// Iterate through each subgraph with the provided closure, returning the
    /// control flow `Break` value if there was an early return.
    pub fn for_each_until<T>(
        self,
        graph: &G,
        mut f: impl FnMut(IntegerNodeIndex) -> ControlFlow<G::EdgeWeight, T>,
    ) -> Option<T> {
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
pub enum ControlFlow<W, T> {
    /// Continue the iterating through the subgraphs with the provided
    /// predecessor vector indicating which nodes are connected to the current
    /// subgraph.
    Continue(ShortestPathGraph<W>),
    /// Stop iterating through the subgraphs and return a result.
    Break(T),
}
