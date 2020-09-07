use anyhow::Result;
use prometheus::{Histogram, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry};
use std::time::Instant;
use warp::log::Info;

// There are global metrics (response_status and response_time) that are measured through warp's log
// functionality. The global metrics work on the final response and do not know about the specific
// route that handled them.
// And there are metrics (response_time_success) that are labelled per route. We only measures the
// success response time here as error response times are short and uninteresting and would skew the
// result.

pub struct Metrics {
    response_status: IntCounterVec,
    response_time: Histogram,
    response_time_per_route: HistogramVec,
}

impl Metrics {
    pub fn new(registry: &Registry) -> Result<Self> {
        let opts = Opts::new(
            "price_estimator_response_status",
            "The status code of a price estimator response.",
        );
        let response_status = IntCounterVec::new(opts, &["status"]).unwrap();
        registry.register(Box::new(response_status.clone()))?;

        let opts = HistogramOpts::new(
            "price_estimator_response_time_global",
            "The duration it takes for the price estimator to respond.",
        );
        let response_time = Histogram::with_opts(opts).unwrap();
        registry.register(Box::new(response_time.clone()))?;

        let opts = HistogramOpts::new(
            "price_estimator_response_time_per_route",
            "The duration it takes for the price estimator to successfully respond for each route.",
        );
        let response_time_per_route = HistogramVec::new(opts, &["route"]).unwrap();
        registry.register(Box::new(response_time_per_route.clone()))?;

        Ok(Self {
            response_status,
            response_time,
            response_time_per_route,
        })
    }

    pub fn handle_successful_response(&self, route: &str, start: Instant) {
        let response_time = start.elapsed().as_secs_f64();
        self.response_time_per_route
            .with_label_values(&[route])
            .observe(response_time);
    }

    pub fn handle_response(&self, info: Info<'_>) {
        let status = info.status();
        self.response_status
            .with_label_values(&[status.as_str()])
            .inc();
        let response_time = info.elapsed().as_secs_f64();
        self.response_time.observe(response_time);
    }
}
