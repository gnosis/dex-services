//! This module contains graph related data types and algorithms that are either
//! not included in the `petgraph` crate or are modified versions of the crate's
//! implementation.

pub mod bellman_ford;
pub mod path;
pub mod subgraph;

pub type IntegerNodeIndex = usize;