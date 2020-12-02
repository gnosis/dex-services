//! Module implementing tools for iterating over disconnected subgraphs.

use std::cmp::Ord;
use std::collections::BTreeSet;

/// A struct used for iterating over disconnected subgraphs in the orderbook for
/// detecting orderbook overlaps and reducing the orderbook.
///
/// Note that this pseudo-iterator uses a `BTreeSet` to ensure that subgraphs
/// are visited in a predictable order starting the from the first node.
pub struct Subgraphs<N>(BTreeSet<N>);

impl<N: Copy + Ord> Subgraphs<N> {
    /// Create a new subgraphs iterator from an iterator of nodes.
    pub fn new(nodes: impl Iterator<Item = N>) -> Self {
        Subgraphs::<N>(nodes.collect())
    }

    /// Iterate through each subgraph with the provided closure, returning the
    /// control flow `Break` value if there was an early return.
    pub fn for_each_until<T>(self, mut f: impl FnMut(N) -> ControlFlow<N, T>) -> Option<T> {
        let Self(mut remaining_tokens) = self;
        while let Some(&token) = remaining_tokens.iter().next() {
            remaining_tokens.remove(&token);
            let connected_nodes = match f(token) {
                ControlFlow::Continue(connected_nodes) => connected_nodes,
                ControlFlow::Break(result) => return Some(result),
            };
            for connected in connected_nodes {
                remaining_tokens.remove(&connected);
            }
        }

        None
    }
}

/// An enum for representing control flow when iterating subgraphs.
pub enum ControlFlow<N, T> {
    /// Continue the iterating through the subgraphs with the provided
    /// connected component of the graph.
    Continue(Vec<N>),
    /// Stop iterating through the subgraphs and return a result.
    Break(T),
}
