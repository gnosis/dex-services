use prometheus::{Gauge, Opts, Registry};

use std::sync::Arc;

pub struct SolverMetrics {
    objective_values: Gauge,
    objective_values_touched_orders: Gauge,
    processing_time: Gauge,
    optimality_gap: Gauge,
}

impl SolverMetrics {
    pub fn new(registry: Arc<Registry>) -> Self {
        let objective_values = Gauge::new(
            "dfusion_solver_objective_values",
            "objective value of solution",
        )
        .unwrap();
        registry
            .register(Box::new(objective_values.clone()))
            .unwrap();

        let objective_values_touched_orders = Gauge::new(
            "dfusion_solver_objective_values_touched",
            "objective value for touched orders of solution",
        )
        .unwrap();
        registry
            .register(Box::new(objective_values_touched_orders.clone()))
            .unwrap();

        let processing_time = Gauge::new(
            "dfusion_solver_processing_times",
            "execution time of solver",
        )
        .unwrap();
        registry
            .register(Box::new(processing_time.clone()))
            .unwrap();
        let optimality_gap = Gauge::new(
            "dfusion_solver_processing_times",
            "execution time of solver",
        )
        .unwrap();
        registry.register(Box::new(optimality_gap.clone())).unwrap();
        Self {
            objective_values,
            objective_values_touched_orders,
            processing_time,
            optimality_gap,
        }
    }
}
