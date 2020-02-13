pub mod error;
pub mod naive_solver;
pub mod optimization_price_finder;
pub mod price_finder_interface;

pub use crate::price_finding::naive_solver::NaiveSolver;
pub use crate::price_finding::optimization_price_finder::OptimisationPriceFinder;
pub use crate::price_finding::price_finder_interface::{Fee, OptimizationModel, PriceFinding};
use log::info;

/// Creates a price finding model for the given parameters.
pub fn create_price_finder(
    fee: Option<Fee>,
    optimization_model: OptimizationModel,
) -> Box<dyn PriceFinding> {
    if optimization_model == OptimizationModel::NAIVE {
        info!("Using naive price finder");
        Box::new(NaiveSolver::new(fee))
    } else {
        info!(
            "Using optimisation price finder with the args {:}",
            optimization_model.to_args()
        );
        Box::new(OptimisationPriceFinder::new(fee, optimization_model))
    }
}
