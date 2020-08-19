use crate::models::solution::SolverStats;
use prometheus::{Gauge, Registry};
use std::sync::Arc;

pub struct SolverMetrics {
    volume: Gauge,
    utility: Gauge,
    utility_disreg: Gauge,
    utility_disreg_touched: Gauge,
    fees: Gauge,
    orders_touched: Gauge,
    runtime: Gauge,
    runtime_preprocessing: Gauge,
    runtime_solving: Gauge,
    runtime_ring_finding: Gauge,
    runtime_validation: Gauge,
    nr_variables: Gauge,
    nr_bool_variables: Gauge,
    optimality_gap: Gauge,
    obj_val: Gauge,
    obj_val_sc: Gauge,
}

impl SolverMetrics {
    pub fn new(registry: Arc<Registry>) -> Self {
        let make_gauge = |name| {
            let name = format!("dfusion_solver_{}", name);
            // We leave the help string empty because the metrics should be documented in the solver
            // repository instead of here. However the prometheus library requires a non empty help
            // string so we use a single space.
            let help = " ".to_string();
            let gauge = Gauge::new(name, help).unwrap();
            registry.register(Box::new(gauge.clone())).unwrap();
            gauge
        };

        macro_rules! create {
            ($($name:ident),*) => {
                Self {
                    $(
                        $name: make_gauge(stringify!($name))
                    ),*
                }
            };
        }

        create! {
            volume,
            utility,
            utility_disreg,
            utility_disreg_touched,
            fees,
            orders_touched,
            runtime,
            runtime_preprocessing,
            runtime_solving,
            runtime_ring_finding,
            runtime_validation,
            nr_variables,
            nr_bool_variables,
            optimality_gap,
            obj_val,
            obj_val_sc
        }
    }

    pub fn handle_stats(&self, stats: &SolverStats) {
        let str_to_f64 = |string| std::str::FromStr::from_str(string).unwrap_or_default();

        self.volume.set(str_to_f64(&stats.obj_vals.volume));
        self.utility.set(str_to_f64(&stats.obj_vals.utility));
        self.utility_disreg
            .set(str_to_f64(&stats.obj_vals.utility_disreg));
        self.utility_disreg_touched
            .set(str_to_f64(&stats.obj_vals.utility_disreg_touched));
        self.fees.set(stats.obj_vals.fees as f64);
        self.orders_touched
            .set(stats.obj_vals.orders_touched as f64);
        self.runtime.set(stats.solver.runtime);
        self.runtime_preprocessing
            .set(stats.solver.runtime_preprocessing);
        self.runtime_solving.set(stats.solver.runtime_solving);
        self.runtime_ring_finding
            .set(stats.solver.runtime_ring_finding);
        self.runtime_validation.set(stats.solver.runtime_validation);
        self.nr_variables.set(stats.solver.nr_bool_variables as f64);
        self.nr_bool_variables
            .set(stats.solver.nr_bool_variables as f64);
        self.optimality_gap.set(stats.solver.optimality_gap);
        self.obj_val.set(stats.solver.obj_val);
        self.obj_val_sc.set(stats.solver.obj_val_sc);
    }
}
