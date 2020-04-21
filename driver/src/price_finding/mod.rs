pub mod naive_solver;
pub mod optimization_price_finder;
pub mod price_finder_interface;

use crate::price_estimation::PriceEstimating;
pub use crate::price_finding::naive_solver::NaiveSolver;
pub use crate::price_finding::optimization_price_finder::OptimisationPriceFinder;
pub use crate::price_finding::price_finder_interface::{Fee, PriceFinding, SolverConfig};
use log::info;

pub fn create_price_finder(
    fee: Option<Fee>,
    solver_config: SolverConfig,
    price_oracle: impl PriceEstimating + Sync + 'static,
) -> Box<dyn PriceFinding + Sync> {
    if solver_config == SolverConfig::NaiveSolver {
        info!("Using naive price finder");
        Box::new(NaiveSolver::new(fee))
    } else {
        info!("Using {:?} optimization price finder", solver_config);
        Box::new(OptimisationPriceFinder::new(
            fee,
            solver_config,
            price_oracle,
        ))
    }
}
