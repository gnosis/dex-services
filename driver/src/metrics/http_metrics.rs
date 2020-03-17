use anyhow::Result;
use prometheus::{HistogramOpts, HistogramVec, Registry};
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
            vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0],
        )?;
        let size = HttpMetrics::initialize_histogram(
            registry,
            "dfusion_service_http_size",
            "Size in bytes for HTTP response bodies",
            vec![
                1_000.0,
                10_000.0,
                100_000.0,
                1_000_000.0,
                10_000_000.0,
                100_000_000.0,
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
        let histogram = HistogramVec::new(options, &["client", "request"])?;

        Web3Label::initialize_histogram_labels(&histogram);
        KrakenLabel::initialize_histogram_labels(&histogram);
        DexagLabel::initialize_histogram_labels(&histogram);
        GasStationLabel::initialize_histogram_labels(&histogram);

        registry.register(Box::new(histogram.clone()))?;

        Ok(histogram)
    }

    /// Add a request latency and size measurement to the current HTTP metrics
    /// registry for the specified label values.
    pub fn request<L: LabelValues>(&self, label: L, latency: Duration, size: usize) {
        self.latency
            .with_label_values(&label.label_values())
            .observe(latency.as_secs_f64());
        self.size
            .with_label_values(&label.label_values())
            .observe(size as _);
    }
}

impl Default for HttpMetrics {
    fn default() -> Self {
        HttpMetrics::new(&Default::default()).unwrap()
    }
}

/// A common trait shared between labeled and unlabeled subsystems for getting
/// the label values to use for metrics.
pub trait LabelValues {
    fn label_values(&self) -> [&'static str; 2];
    fn initialize_histogram_labels(histograms: &HistogramVec);
}

/// A trait abstracting a labeled subsystem, that is a specific use of an HTTP
/// client where the HTTP requests should be differenciated.
pub trait LabeledSubsystem: LabelValues + Sized {
    fn name() -> &'static str;
    fn all_labels() -> &'static [Self];
    fn label(&self) -> &'static str;
}

/// A trait abstracting a labeled subsystem, that is a specific use of an HTTP
/// client where the HTTP requests should not be differenciated.
pub trait UnlabeledSubsystem: Default + LabelValues {
    fn name() -> &'static str;
}

macro_rules! subsystem {
    (
        $(#[$attr:meta])*
        pub enum $name:ident as $n:tt {
            $($variant:ident => $label:tt,)*
        }
    ) => {
        $(#[$attr])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum $name {
            $(
                $variant,
            )*
        }

        impl LabelValues for $name {
            fn label_values(&self) -> [&'static str; 2] {
                [Self::name(), self.label()]
            }

            fn initialize_histogram_labels(histograms: &HistogramVec) {
                for label in Self::all_labels() {
                    histograms.with_label_values(&label.label_values());
                }
            }
        }

        impl LabeledSubsystem for $name {
            fn name() -> &'static str {
                $n
            }

            fn all_labels() -> &'static [Self] {
                const ALL: &[$name] = &[$($name::$variant),*];
                ALL
            }

            fn label(&self) -> &'static str {
                match self {
                    $(
                        $name::$variant => $label,
                    )*
                }
            }
        }
    };

    (
        $(#[$attr:meta])*
        pub struct $name:ident as $n:tt;
    ) => {
        $(#[$attr])*
        #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
        pub struct $name;

        impl LabelValues for $name {
            fn label_values(&self) -> [&'static str; 2] {
                [Self::name(), ""]
            }

            fn initialize_histogram_labels(histograms: &HistogramVec) {
                histograms.with_label_values(&Self::default().label_values());
            }
        }

        impl UnlabeledSubsystem for $name {
            fn name() -> &'static str {
                $n
            }
        }
    };
}

subsystem! {
    /// A label for `web3` HTTP requests. This subsystem is used by the HTTP
    /// transport implementation.
    pub enum Web3Label as "web3" {
        Call => "call",
        EstimateGas => "estimate_gas",
        Other => "other",
    }
}

subsystem! {
    /// A label for Kraken HTTP API requests. This subsystem is used by the
    /// Kraken API implementation.
    pub struct KrakenLabel as "kraken";
}

subsystem! {
    /// A label for Dexag HTTP API requests. This subsystem is used by the Dexag
    /// API implementation.
    pub struct DexagLabel as "dexag";
}

subsystem! {
    /// A label for Gnosis Safe gas station HTTP API request. This subsystem is
    /// used by the gas station client implementation.
    pub struct GasStationLabel as "gas_station";
}
