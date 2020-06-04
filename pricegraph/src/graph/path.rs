//! Utilities for finding paths from predecessor vectors.

use petgraph::graph::NodeIndex;

/// Finds a cycle starting at the provided `search` node and returns a vector
/// representing a path along the cycle.
///
/// Optionally, an `origin` node can be provided so that the first element of
/// the cycle vector is `origin` if and only if `origin` is part of the cycle.
///
/// Returns `None` if no such cycle can be found.
pub fn find_cycle(
    predecessor: &[Option<NodeIndex>],
    search: NodeIndex,
    origin: Option<NodeIndex>,
) -> Option<Vec<NodeIndex>> {
    // NOTE: First find a node that is actually on the cycle, this is done
    // because a negative cycle can be detected on any node connected to the
    // cycle and not just nodes on the cycle itself.
    let mut visited = vec![0; predecessor.len()];
    let mut cursor = search;
    let mut step = 1;
    visited[cursor.index()] = step;
    loop {
        cursor = predecessor[cursor.index()]?;
        if visited[cursor.index()] > 0 {
            break;
        }
        step += 1;
        visited[cursor.index()] = step;
    }

    // NOTE: Allocate the cycle vector with enough capacity for the negative
    // cycle plus one. This extra capacity is used by the orderbook graph to
    // create a circular path equivalent to the negative cycle.
    let len = step + 1 - visited[cursor.index()];
    let mut path = Vec::with_capacity(len + 1);

    // NOTE: `cursor` is now guaranteed to be on the cycle. Furthermore, if
    // `origin` was visited after `cursor`, then it is on the cycle as well.
    let start = match origin {
        Some(origin) if visited[origin.index()] > visited[cursor.index()] => origin,
        _ => cursor,
    };

    // NOTE: Now we have found the cycle starting at `start`, walk backwards
    // until we reach the `start` node again.
    let mut cursor = start;
    loop {
        cursor = predecessor[cursor.index()]?;
        path.push(cursor);
        if cursor == start {
            break;
        }
    }

    // NOTE: `path` is in reverse order, since it was built by walking the cycle
    // backwards, so reverse it and done!
    path.reverse();
    Some(path)
}

/// Finds a path between two tokens. Returns `None` if no such path exists.
pub fn find_path(
    predecessor: &[Option<NodeIndex>],
    start: NodeIndex,
    end: NodeIndex,
) -> Option<Vec<NodeIndex>> {
    let mut path = Vec::with_capacity(predecessor.len());

    let mut current = end;
    while current != start {
        path.push(current);
        current = predecessor[current.index()]?;
    }
    path.push(start);

    // NOTE: `path` is in reverse order, since it was built by walking the path
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

        let NegativeCycle(predecessor, node) = bellman_ford::search(&graph, 0.into()).unwrap_err();

        let cycle = find_cycle(&predecessor, node, None).unwrap();
        assert_eq!(cycle, &[1.into(), 2.into(), 3.into()]);
        assert_eq!(cycle.capacity(), cycle.len() + 1);

        let cycle = find_cycle(&predecessor, node, Some(2.into())).unwrap();
        assert_eq!(cycle, &[2.into(), 3.into(), 1.into()]);

        // NOTE: if `origin` is provided, but not part of the cycle, then the
        // first node in the vector cycle can be any node.
        let cycle = find_cycle(&predecessor, node, Some(4.into())).unwrap();
        assert_eq!(cycle, &[1.into(), 2.into(), 3.into()]);
    }

    #[test]
    fn search_finds_shortest_path() {
        //  0 --2.0-> 1 --1.0-> 2
        //  |         |         |
        // 4.0       7.0        |
        //  v         v         |
        //  3         5        5.0
        //  |         ^         |
        // 1.0       1.0        |
        //  |         |         |
        //  \-------> 4 <-------/
        let graph = Graph::<(), f64>::from_edges(&[
            (0, 1, 2.0),
            (0, 3, 4.0),
            (1, 2, 1.0),
            (1, 5, 7.0),
            (2, 4, 5.0),
            (4, 5, 1.0),
            (3, 4, 1.0),
        ]);

        let (_, predecessor) = bellman_ford::search(&graph, 0.into()).unwrap();
        let path = find_path(&predecessor, 0.into(), 5.into()).unwrap();

        assert_eq!(path, &[0.into(), 3.into(), 4.into(), 5.into()]);
    }
}
