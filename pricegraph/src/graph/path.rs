//! Definitions of paths and cycles of a graph.

use petgraph::visit::NodeIndexable;
use std::ops::Deref;

#[derive(Debug, PartialEq, Eq)]
/// A path of nodes connected by a (directed) edge.
pub struct Path<N>(pub Vec<N>);

impl<N> Deref for Path<N> {
    type Target = [N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<N> From<NegativeCycle<N>> for Path<N> {
    fn from(cycle: NegativeCycle<N>) -> Self {
        Path(cycle.0)
    }
}

#[derive(Clone, Debug)]
/// An ordered collection of nodes that form a cycle of negative weight.
/// The first node of the cycle coincides with the last.
pub struct NegativeCycle<N>(pub Vec<N>);

impl<N> Deref for NegativeCycle<N> {
    type Target = [N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<N: Clone + PartialEq> NegativeCycle<N> {
    /// Returns the negative cycle changing its starting and terminating
    /// node to be the given node. If the given node is not part of the
    /// cycle, it returns an error containing the original cycle.
    pub fn with_starting_node(mut self, start: N) -> Result<Self, Self> {
        match self.0.iter().position(|i| *i == start) {
            None => Err(self),
            Some(pos) if pos == 0 => Ok(self),
            Some(pos) => {
                let popped = self.0.pop();
                debug_assert!(popped.as_ref() == self.first());
                self.0.rotate_left(pos);
                debug_assert!(self.0[0] == start);
                self.0.push(start);
                Ok(self)
            }
        }
    }

    /// Returns two paths: from the start to the given index and from
    /// the given index to the end of the cycle.
    pub fn split_at(self, node: N) -> Result<(Path<N>, Path<N>), Self> {
        if let Some(index) = self.iter().position(|entry| *entry == node) {
            let (start_to_index, index_to_end) = self.0.split_at(index);
            let mut start_to_index_vec = start_to_index.to_vec();
            start_to_index_vec.push(node);
            Ok((Path(start_to_index_vec), Path(index_to_end.to_vec())))
        } else {
            Err(self)
        }
    }
}

/// Finds a negative cycle by searching from the provided `search` node.
///
/// Optionally, an `origin` node can be provided so that the first element of
/// the cycle vector is `origin` if and only if `origin` is part of the cycle.
///
/// Returns `None` if no cycle can be found.
pub fn find_cycle<G: NodeIndexable>(
    graph: G,
    predecessor: &[Option<G::NodeId>],
    search: G::NodeId,
    origin: Option<G::NodeId>,
) -> Option<NegativeCycle<G::NodeId>> {
    // NOTE: First find a node that is actually on the cycle, this is done
    // because a negative cycle can be detected on any node connected to the
    // cycle and not just nodes on the cycle itself.
    let mut visited = vec![0; predecessor.len()];
    let mut cursor = search;
    let mut step = 1;
    visited[graph.to_index(cursor)] = step;
    loop {
        cursor = predecessor[graph.to_index(cursor)]?;
        if visited[graph.to_index(cursor)] > 0 {
            break;
        }
        step += 1;
        visited[graph.to_index(cursor)] = step;
    }

    // NOTE: Allocate the cycle vector with enough capacity for the negative
    // cycle path, that is the length of the negative cycle plus one (which is
    // used by the final segment of the path to return to the starting node).
    let len = step + 1 - visited[graph.to_index(cursor)];
    let mut path = Vec::with_capacity(len + 1);

    // NOTE: `cursor` is now guaranteed to be on the cycle. Furthermore, if
    // `origin` was visited after `cursor`, then it is on the cycle as well.
    let start = match origin {
        Some(origin) if visited[graph.to_index(origin)] > visited[graph.to_index(cursor)] => origin,
        _ => cursor,
    };

    // NOTE: Now we have found the cycle starting at `start`, walk backwards
    // until we reach the `start` node again.
    let mut cursor = start;
    path.push(cursor);
    loop {
        cursor = predecessor[graph.to_index(cursor)]?;
        path.push(cursor);
        if cursor == start {
            break;
        }
    }

    // NOTE: `path` is in reverse order, since it was built by walking the cycle
    // backwards, so reverse it and done!
    path.reverse();
    Some(NegativeCycle(path))
}
