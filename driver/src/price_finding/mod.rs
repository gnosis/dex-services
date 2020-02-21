pub mod naive_solver;
pub mod optimization_price_finder;
pub mod price_finder_interface;

use crate::models::TokenData;
pub use crate::price_finding::naive_solver::NaiveSolver;
pub use crate::price_finding::optimization_price_finder::OptimisationPriceFinder;
pub use crate::price_finding::price_finder_interface::{Fee, PriceFinding, SolverType};
use log::info;

pub fn create_price_finder(
    fee: Option<Fee>,
    solver_type: SolverType,
    token_data: TokenData,
) -> Box<dyn PriceFinding> {
    if solver_type == SolverType::NaiveSolver {
        info!("Using naive price finder");
        Box::new(NaiveSolver::new(fee))
    } else {
        info!(
            "Using optimisation price finder with the args {:}",
            solver_type.to_args()
        );
        Box::new(OptimisationPriceFinder::new(fee, solver_type, token_data))
    }
}
