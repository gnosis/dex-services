pub mod naive_solver;
pub mod optimization_price_finder;
pub mod price_finder_interface;

pub use self::{
    naive_solver::NaiveSolver,
    optimization_price_finder::OptimisationPriceFinder,
    price_finder_interface::{Fee, InternalOptimizer, PriceFinding, SolverType},
};
use crate::{economic_viability::EconomicViabilityComputing, price_estimation::PriceEstimating};
use log::info;
use std::sync::Arc;

pub fn create_price_finder(
    fee: Option<Fee>,
    solver_type: SolverType,
    price_oracle: Arc<dyn PriceEstimating + Send + Sync>,
    min_avg_fee: Arc<dyn EconomicViabilityComputing>,
    internal_optimizer: InternalOptimizer,
) -> Box<dyn PriceFinding + Sync> {
    if solver_type == SolverType::NaiveSolver {
        info!("Using naive price finder");
        Box::new(NaiveSolver::new(fee))
    } else {
        info!("Using {:?} optimization price finder", solver_type);
        Box::new(OptimisationPriceFinder::new(
            fee,
            solver_type,
            price_oracle,
            min_avg_fee,
            internal_optimizer,
        ))
    }
}
