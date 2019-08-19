#[macro_use]
extern crate log;

pub mod database;
pub mod models;

use graph::prelude::SubgraphDeploymentId;

#[macro_use]
extern crate lazy_static;

pub const SUBGRAPH_NAME: &str = "dfusion";

lazy_static! {
    static ref SUBGRAPH_ID: SubgraphDeploymentId =
        SubgraphDeploymentId::new(SUBGRAPH_NAME).unwrap();
}
