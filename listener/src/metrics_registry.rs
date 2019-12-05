use graph::components::metrics::{
    Collector, Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramOpts, HistogramVec, Opts,
    PrometheusError,
};
use graph::prelude::MetricsRegistry;
use std::collections::HashMap;

#[derive(Clone)]
pub struct SimpleMetricsRegistry;

impl MetricsRegistry for SimpleMetricsRegistry {
    fn new_gauge(
        &self,
        name: String,
        help: String,
        const_labels: HashMap<String, String>,
    ) -> Result<Box<Gauge>, PrometheusError> {
        eprintln!("new_gauge");
        let opts = Opts::new(name, help).const_labels(const_labels);
        let gauge = Box::new(Gauge::with_opts(opts)?);
        Ok(gauge)
    }

    fn new_gauge_vec(
        &self,
        name: String,
        help: String,
        const_labels: HashMap<String, String>,
        variable_labels: Vec<String>,
    ) -> Result<Box<GaugeVec>, PrometheusError> {
        eprintln!("new_gauge_vec");
        let opts = Opts::new(name, help).const_labels(const_labels);
        let gauges = Box::new(GaugeVec::new(
            opts,
            variable_labels
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>()
                .as_slice(),
        )?);
        Ok(gauges)
    }

    fn new_counter(
        &self,
        name: String,
        help: String,
        const_labels: HashMap<String, String>,
    ) -> Result<Box<Counter>, PrometheusError> {
        eprintln!("new_counter");
        let opts = Opts::new(name, help).const_labels(const_labels);
        let counter = Box::new(Counter::with_opts(opts)?);
        Ok(counter)
    }

    fn new_counter_vec(
        &self,
        name: String,
        help: String,
        const_labels: HashMap<String, String>,
        variable_labels: Vec<String>,
    ) -> Result<Box<CounterVec>, PrometheusError> {
        eprintln!("new_counter_vec");
        let opts = Opts::new(name, help).const_labels(const_labels);
        let counters = Box::new(CounterVec::new(
            opts,
            variable_labels
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>()
                .as_slice(),
        )?);
        Ok(counters)
    }

    fn new_histogram(
        &self,
        name: String,
        help: String,
        const_labels: HashMap<String, String>,
        buckets: Vec<f64>,
    ) -> Result<Box<Histogram>, PrometheusError> {
        eprintln!("new_histogram");
        let opts = HistogramOpts::new(name, help)
            .const_labels(const_labels)
            .buckets(buckets);
        let histogram = Box::new(Histogram::with_opts(opts)?);
        Ok(histogram)
    }

    fn new_histogram_vec(
        &self,
        name: String,
        help: String,
        const_labels: HashMap<String, String>,
        variable_labels: Vec<String>,
        buckets: Vec<f64>,
    ) -> Result<Box<HistogramVec>, PrometheusError> {
        eprintln!("new_histogram_vec");
        let opts = Opts::new(name, help).const_labels(const_labels);
        let histogram = Box::new(HistogramVec::new(
            HistogramOpts {
                common_opts: opts,
                buckets,
            },
            variable_labels
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<&str>>()
                .as_slice(),
        )?);
        Ok(histogram)
    }

    fn unregister(&self, _: Box<dyn Collector>) {
        return;
    }
}
