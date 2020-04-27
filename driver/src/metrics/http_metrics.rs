use anyhow::Result;
use ethcontract::jsonrpc::types::{Call, Request};
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
            DEFAULT_BUCKETS.to_vec(),
        )?;
        let size = HttpMetrics::initialize_histogram(
            registry,
            "dfusion_service_http_size",
            "Size in bytes for HTTP response bodies",
            prometheus::exponential_buckets(100.0, 10.0, 8)?,
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
        EthBatchRPC => "eth_batch_rpc",
        Kraken => "kraken",
        Dexag => "dexag",
        GasStation => "gas_station",
    }
}

impl From<&Request> for HttpLabel {
    fn from(request: &Request) -> Self {
        match request {
            Request::Single(call) => match &call {
                Call::MethodCall(call) if call.method == "eth_call" => HttpLabel::EthCall,
                Call::MethodCall(call) if call.method == "eth_estimateGas" => {
                    HttpLabel::EthEstimateGas
                }
                _ => HttpLabel::EthRpc,
            },
            Request::Batch(_) => HttpLabel::EthBatchRPC,
        }
    }
}
