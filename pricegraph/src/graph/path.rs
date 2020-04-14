//! Utilities for finding paths from predecessor vectors.

use petgraph::graph::NodeIndex;

/// Finds a cycle and returns a vector representing a path along the cycle,
/// ending that is the predecessor of the starting node.
///
/// Returns `None` if no such cycle can be found.
pub fn find_cycle(predecessors: &[Option<NodeIndex>], start: NodeIndex) -> Option<Vec<NodeIndex>> {
    // NOTE: First find a node that is actually on the cycle, this is done
    // because a negative cycle can be detected on any node connected to the
    // cycle and not just nodes on the cycle itself.
    let mut visited = vec![false; predecessors.len()];
    let mut current = start;
    visited[current.index()] = true;
    loop {
        current = predecessors[current.index()]?;
        if visited[current.index()] {
            break;
        }
        visited[current.index()] = true;
    }

    // NOTE: `current` is now guaranteed to be on the cycle, so just walk
    // backwards until we reach `current` again.
    let start = current;
    let mut path = Vec::with_capacity(predecessors.len());
    loop {
        current = predecessors[current.index()]?;
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
    use crate::graph::bellman_ford::{self, NegativeCycle};
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

        let NegativeCycle(predecessors, node) = bellman_ford::search(&graph, 0.into()).unwrap_err();
        let cycle = find_cycle(&predecessors, node).unwrap();

        assert_eq!(cycle, &[1.into(), 2.into(), 3.into()]);
    }
}
