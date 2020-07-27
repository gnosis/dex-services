pub mod naive_solver;
pub mod optimization_price_finder;
pub mod price_finder_interface;

pub use self::{
    naive_solver::NaiveSolver,
    optimization_price_finder::OptimisationPriceFinder,
    price_finder_interface::{Fee, InternalOptimizer, PriceFinding, SolverType},
};
use crate::{gas_station::GasPriceEstimating, price_estimation::PriceEstimating};
use log::info;
use std::sync::Arc;

pub fn create_price_finder(
    fee: Option<Fee>,
    solver_type: SolverType,
    price_oracle: impl PriceEstimating + Send + Sync + 'static,
    gas_station: Arc<dyn GasPriceEstimating + Send + Sync>,
    min_avg_fee_subsidy_factor: f64,
    default_min_avg_fee_per_order: u128,
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
            gas_station,
            min_avg_fee_subsidy_factor,
            default_min_avg_fee_per_order,
            internal_optimizer,
        ))
    }
}
