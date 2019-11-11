pub mod database;
pub mod models;

use graph::prelude::SubgraphDeploymentId;

use lazy_static::lazy_static;

pub const SUBGRAPH_NAME: &str = "dfusion";

lazy_static! {
    static ref SUBGRAPH_ID: SubgraphDeploymentId =
        SubgraphDeploymentId::new(SUBGRAPH_NAME).unwrap();
}
