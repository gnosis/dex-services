//! Utilities for finding paths from predecessor vectors.

use petgraph::graph::NodeIndex;
use petgraph::visit::NodeIndexable;

/// Finds a cycle by searching from the provided `search` node and returns a
/// vector representing a path along the cycle. This method returns a path,
/// which means that the first and last nodes will always be the same.
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
) -> Option<Vec<G::NodeId>> {
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
    //use super::*;
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

        let search_result = bellman_ford::search(&graph, 0.into(), None);
        let cycle = match search_result {
            Err(NegativeCycle(cycle)) => cycle,
            _ => panic!("Negative cycle not found"),
        };

        assert_eq!(cycle, &[1.into(), 2.into(), 3.into(), 1.into()]);

        let cycle = cycle.change_starting_node(2.into()).unwrap();
        assert_eq!(cycle, &[2.into(), 3.into(), 1.into(), 2.into()]);

        // NOTE: if `origin` is provided, but not part of the cycle, then the
        // first node in the vector cycle can be any node.
        let cycle = cycle.change_starting_node(4.into()).unwrap_err();
        assert_eq!(cycle, &[1.into(), 2.into(), 3.into(), 1.into()]);
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

        let shortest_path_graph = bellman_ford::search(&graph, 0.into(), None).unwrap();
        let path = shortest_path_graph.find_path_to(5.into()).unwrap();

        assert_eq!(path, &[0.into(), 3.into(), 4.into(), 5.into()]);
    }
}
