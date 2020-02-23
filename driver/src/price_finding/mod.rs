pub mod naive_solver;
pub mod optimization_price_finder;
pub mod price_finder_interface;

use crate::models::TokenData;
pub use crate::price_finding::naive_solver::NaiveSolver;
pub use crate::price_finding::optimization_price_finder::OptimisationPriceFinder;
pub use crate::price_finding::price_finder_interface::{Fee, PriceFinding, SolverConfig};
use log::info;

pub fn create_price_finder(
    fee: Option<Fee>,
    solver_type: SolverConfig,
    token_data: TokenData,
) -> Box<dyn PriceFinding> {
    if solver_type == SolverConfig::NaiveSolver {
        info!("Using naive price finder");
        Box::new(NaiveSolver::new(fee))
    } else {
        info!(
            "Using optimization price finder with the args {:?}",
            solver_type.to_args()
        );
        Box::new(OptimisationPriceFinder::new(fee, solver_type, token_data))
    }
}
