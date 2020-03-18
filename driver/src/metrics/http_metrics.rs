use anyhow::Result;
use prometheus::{HistogramOpts, HistogramVec, Registry, DEFAULT_BUCKETS};
use std::sync::Arc;
use std::time::Duration;

/// A registry for all HTTP related metrics.
#[derive(Debug)]
pub struct HttpMetrics {
    latency: HistogramVec,
    size: HistogramVec,
}

impl HttpMetrics {
    /// Create a new HTTP metrics registry.
    pub fn new(registry: &Arc<Registry>) -> Result<Self> {
        let latency = HttpMetrics::initialize_histogram(
            registry,
            "dfusion_service_http_latency",
            "Latency in seconds for HTTP request",
            Vec::from(&DEFAULT_BUCKETS[..]),
        )?;
        let size = HttpMetrics::initialize_histogram(
            registry,
            "dfusion_service_http_size",
            "Size in bytes for HTTP response bodies",
            vec![
                100.0,
                1_000.0,
                10_000.0,
                100_000.0,
                1_000_000.0,
                10_000_000.0,
                100_000_000.0,
                1_000_000_000.0,
            ],
        )?;

        Ok(HttpMetrics { latency, size })
    }

    /// Initializes a histogram with for all the labels.
    fn initialize_histogram(
        registry: &Arc<Registry>,
        name: &str,
        description: &str,
        buckets: Vec<f64>,
    ) -> Result<HistogramVec> {
        let options = HistogramOpts::new(name, description).buckets(buckets);
        let histogram = HistogramVec::new(options, &["request"])?;
        for label in HttpLabel::all_labels() {
            histogram.with_label_values(&label.values());
        }

        registry.register(Box::new(histogram.clone()))?;

        Ok(histogram)
    }

    /// Add a request latency and size measurement to the current HTTP metrics
    /// registry for the specified label value.
    pub fn request(&self, label: HttpLabel, latency: Duration, size: usize) {
        self.latency
            .with_label_values(&label.values())
            .observe(latency.as_secs_f64());
        self.size
            .with_label_values(&label.values())
            .observe(size as _);
    }
}

impl Default for HttpMetrics {
    fn default() -> Self {
        HttpMetrics::new(&Default::default()).unwrap()
    }
}

macro_rules! labels {
    (
        $(#[$attr:meta])*
        pub enum $name:ident {
            $($variant:ident => $label:tt,)*
        }
    ) => {
        $(#[$attr])*
        pub enum $name {
            $(
                $variant,
            )*
        }

        impl $name {
            fn all_labels() -> &'static [Self] {
                const ALL: &[$name] = &[$($name::$variant),*];
                ALL
            }

            fn values(&self) -> &[&str] {
                match self {
                    $(
                        $name::$variant => &[$label],
                    )*
                }
            }
        }
    };
}

labels! {
    /// An enum representing possible HTTP requests for which metrics are being
    /// recorded.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum HttpLabel {
        EthCall => "eth_call",
        EthEstimateGas => "eth_estimate_gas",
        EthRpc => "eth_rpc",
        Kraken => "kraken",
        Dexag => "dexag",
        GasStation => "gas_station",
    }
}
