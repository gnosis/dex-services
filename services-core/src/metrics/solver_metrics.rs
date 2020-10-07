use prometheus::{Gauge, IntCounter, Registry};
use serde::Deserialize;
use serde_json::{json, Number, Value};
use std::{collections::HashMap, sync::Arc};

/// This struct deserializes the metrics part of the solver generated solution json file.
/// We use `default` and serialize to HashMap<String, Value> so that we don't run into errors when
/// the metrics are missing or the format changes. We don't want those errors to prevent the
/// solution from being used.
/// This is also useful to ignore differences in the metrics between solvers. For example the open
/// solver creates less metrics than the others and it serializes `fees` and `orders_touched` as
/// strings instead of numbers.
#[derive(Debug, Deserialize, PartialEq)]
pub struct SolverStats {
    #[serde(rename = "objVals")]
    #[serde(default)]
    pub obj_vals: HashMap<String, Value>,
    #[serde(default)]
    pub solver: HashMap<String, Value>,
}

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
    interrupted: IntCounter,
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

        let interrupted = IntCounter::new(
            "dfusion_solver_interrupted",
            "Increments when solving ran out of time",
        )
        .unwrap();
        registry.register(Box::new(interrupted.clone())).unwrap();

        macro_rules! create {
            ($($name:ident),*) => {
                Self {
                    $(
                        $name: make_gauge(stringify!($name))
                    ),*,
                    interrupted,
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

    /// If a metric is not found in the solution file or cannot be converted to a float, it is set
    /// to 0.
    pub fn handle_stats(&self, stats: &SolverStats) {
        self.volume.set(f64_or_0(&stats.obj_vals, "volume"));
        self.utility.set(f64_or_0(&stats.obj_vals, "utility"));
        self.utility_disreg
            .set(f64_or_0(&stats.obj_vals, "utility_disreg"));
        self.utility_disreg_touched
            .set(f64_or_0(&stats.obj_vals, "utility_disreg_touched"));
        self.fees.set(f64_or_0(&stats.obj_vals, "fees"));
        self.orders_touched
            .set(f64_or_0(&stats.obj_vals, "orders_touched"));

        self.runtime.set(f64_or_0(&stats.solver, "runtime"));
        self.runtime_preprocessing
            .set(f64_or_0(&stats.solver, "runtime_preprocessing"));
        self.runtime_solving
            .set(f64_or_0(&stats.solver, "runtime_solving"));
        self.runtime_ring_finding
            .set(f64_or_0(&stats.solver, "runtime_ring_finding"));
        self.runtime_validation
            .set(f64_or_0(&stats.solver, "runtime_validation"));
        self.nr_variables
            .set(f64_or_0(&stats.solver, "nr_variables"));
        self.nr_bool_variables
            .set(f64_or_0(&stats.solver, "nr_bool_variables"));
        self.optimality_gap
            .set(f64_or_0(&stats.solver, "optimality_gap"));
        self.obj_val.set(f64_or_0(&stats.solver, "obj_val"));
        self.obj_val_sc.set(f64_or_0(&stats.solver, "obj_val_sc"));

        if stats.solver.get("exit_status") == Some(&json!("interrupted")) {
            self.interrupted.inc();
        }
    }
}

fn number_to_f64(number: &Number) -> f64 {
    number
        .as_f64()
        .or_else(|| number.as_i64().map(|n| n as f64))
        .or_else(|| number.as_u64().map(|n| n as f64))
        .unwrap_or(0.0)
}

fn f64_or_0(values: &HashMap<String, Value>, key: &str) -> f64 {
    match values.get(key) {
        Some(Value::Number(number)) => number_to_f64(number),
        Some(Value::String(string)) => string.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}
