//! Module implementing tools for iterating over disconnected subgraphs.

use petgraph::graph::NodeIndex;
use std::collections::BTreeSet;

/// A struct used for iterating over disconnected subgraphs in the orderbook for
/// detecting orderbook overlaps and reducing the orderbook.
///
/// Note that this pseudo-iterator uses a `BTreeSet` to ensure that subgraphs
/// are visited in a predictable order starting the from the first node.
pub struct Subgraphs(BTreeSet<NodeIndex>);

impl Subgraphs {
    /// Create a new subgraphs iterator from an iterator of node indices.
    pub fn new(nodes: impl Iterator<Item = NodeIndex>) -> Self {
        Subgraphs(nodes.collect())
    }

    /// Iterate through each subgraph with the provided closure returning the
    /// predecessor vector for the current node indicating which nodes are
    /// connected to it.
    pub fn for_each(self, mut f: impl FnMut(NodeIndex) -> Vec<Option<NodeIndex>>) {
        self.for_each_until(|node| <ControlFlow<()>>::Continue(f(node)));
    }

    /// Iterate through each subgraph with the provided closure, returning the
    /// control flow `Break` value if there was an early return.
    pub fn for_each_until<T>(self, mut f: impl FnMut(NodeIndex) -> ControlFlow<T>) -> Option<T> {
        let Self(mut remaining_tokens) = self;
        while let Some(&token) = remaining_tokens.iter().next() {
            remaining_tokens.remove(&token);
            let predecessor = match f(token) {
                ControlFlow::Continue(predecessor) => predecessor,
                ControlFlow::Break(result) => return Some(result),
            };

            for connected in predecessor
                .iter()
                .enumerate()
                .filter_map(|(i, &pre)| pre.map(|_| NodeIndex::new(i)))
            {
                remaining_tokens.remove(&connected);
            }
        }

        None
    }
}

/// An enum for representing control flow when iterating subgraphs.
pub enum ControlFlow<T> {
    /// Continue the iterating through the subgraphs with the provided
    /// predecessor vector indicating which nodes are connected to the current
    /// subgraph.
    Continue(Vec<Option<NodeIndex>>),
    /// Stop iterating through the subgraphs and return a result.
    Break(T),
}
