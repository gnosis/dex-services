pub mod naive_solver;
pub mod optimization_price_finder;
pub mod price_finder_interface;

pub use self::{
    naive_solver::NaiveSolver,
    optimization_price_finder::OptimisationPriceFinder,
    price_finder_interface::{Fee, InternalOptimizer, PriceFinding, SolverType},
};
use crate::{
    metrics::{SolverMetrics, StableXMetrics},
    price_estimation::PriceEstimating,
};
use log::info;
use std::sync::Arc;

pub fn create_price_finder(
    fee: Option<Fee>,
    solver_type: SolverType,
    price_oracle: Arc<dyn PriceEstimating + Send + Sync>,
    internal_optimizer: InternalOptimizer,
    solver_metrics: SolverMetrics,
    stablex_metrics: Arc<StableXMetrics>,
) -> Arc<dyn PriceFinding + Send + Sync> {
    if solver_type == SolverType::NaiveSolver {
        info!("Using naive price finder");
        Arc::new(NaiveSolver::new(fee))
    } else {
        info!("Using {:?} optimization price finder", solver_type);
        Arc::new(OptimisationPriceFinder::new(
            fee,
            solver_type,
            price_oracle,
            internal_optimizer,
            solver_metrics,
            stablex_metrics,
        ))
    }
}
