//! Utilities for finding paths from predecessor vectors.

use petgraph::graph::NodeIndex;
use petgraph::visit::NodeIndexable;
use std::collections::HashMap;
use std::hash::Hash;

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
