use anyhow::Result;
use prometheus::{IntGauge, IntGaugeVec, Opts, Registry};
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use std::time::Duration;

/// A registry for all HTTP related metrics.
#[derive(Debug)]
pub struct HttpMetrics {
    labeled: HashMap<TypeId, SubsystemMetrics<IntGaugeVec>>,
    unlabeled: HashMap<TypeId, SubsystemMetrics<IntGauge>>,
}

/// Metrics related to a single HTTP subsystem. Subsystems are grouped based on
/// what the HTTP client is used for.
pub struct SubsystemMetrics<Gauge> {
    latency: Gauge,
    size: Gauge,
}

impl<Gauge> Debug for SubsystemMetrics<Gauge> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("SubsystemMetrics").finish()
    }
}

impl HttpMetrics {
    /// Create a new HTTP metrics registry.
    pub fn new(registry: &Arc<Registry>) -> Result<Self> {
        let mut metrics = HttpMetrics {
            labeled: HashMap::new(),
            unlabeled: HashMap::new(),
        };

        metrics.initialize_labeled_subsystem::<Web3Label>(registry)?;
        metrics.initialize_labeled_subsystem::<KrakenLabel>(registry)?;
        metrics.initialize_unlabeled_subsystem::<GasStationLabel>(registry)?;

        Ok(metrics)
    }

    /// Add a request latency and size measurement to the current HTTP metrics
    /// registry for the specified label.
    pub fn request_labeled<L: LabeledSubsystem + Any>(
        &self,
        label: L,
        latency: Duration,
        size: usize,
    ) {
        let subsystem = self
            .labeled
            .get(&TypeId::of::<L>())
            .expect("all labeled subsystems have registered metrics");

        subsystem
            .latency
            .with_label_values(&[label.label()])
            .set(latency.as_millis() as _);

        subsystem
            .size
            .with_label_values(&[label.label()])
            .set(size as _);
    }

    /// Add a request latency and size measurement to the current HTTP metrics
    /// registry.
    pub fn request_unlabeled<L: UnlabeledSubsystem + Any>(&self, latency: Duration, size: usize) {
        let subsystem = self
            .unlabeled
            .get(&TypeId::of::<L>())
            .expect("all labeled subsystems have registered metrics");

        subsystem.latency.set(latency.as_millis() as _);
        subsystem.size.set(size as _);
    }

    /// Initialize the metrics for a labeled subsystem.
    fn initialize_labeled_subsystem<L: LabeledSubsystem + Any>(
        &mut self,
        registry: &Arc<Registry>,
    ) -> Result<()> {
        let name = L::name();

        let latency = {
            let opts = Opts::new(
                format!("dfusion_service_http_{}_latency", name),
                format!("Latency in milliseconds for {} HTTP requests", name),
            );
            L::initialize_gauges(opts)?
        };
        registry.register(Box::new(latency.clone()))?;

        let size = {
            let opts = Opts::new(
                format!("dfusion_service_http_{}_size", name),
                format!("Size in bytes of {} HTTP responses", name),
            );
            L::initialize_gauges(opts)?
        };
        registry.register(Box::new(size.clone()))?;

        self.labeled
            .insert(TypeId::of::<L>(), SubsystemMetrics { latency, size });
        Ok(())
    }

    /// Initialize metrics for an unlabeled subsystem.
    fn initialize_unlabeled_subsystem<L: UnlabeledSubsystem + Any>(
        &mut self,
        registry: &Arc<Registry>,
    ) -> Result<()> {
        let name = L::name();

        let latency = {
            let opts = Opts::new(
                format!("dfusion_service_http_{}_latency", name),
                format!("Latency for {} HTTP requests", name),
            );
            let gauge = IntGauge::with_opts(opts)?;
            gauge.set(0);
            gauge
        };
        registry.register(Box::new(latency.clone()))?;

        let size = {
            let opts = Opts::new(
                format!("dfusion_service_http_{}_size", name),
                format!("Size of {} HTTP responses", name),
            );
            let gauge = IntGauge::with_opts(opts)?;
            gauge.set(0);
            gauge
        };
        registry.register(Box::new(size.clone()))?;

        self.unlabeled
            .insert(TypeId::of::<L>(), SubsystemMetrics { latency, size });
        Ok(())
    }
}

impl Default for HttpMetrics {
    fn default() -> Self {
        HttpMetrics::new(&Default::default()).unwrap()
    }
}

/// A trait abstracting a labeled subsystem, that is a specific use of an HTTP
/// client where the HTTP requests should be differenciated.
pub trait LabeledSubsystem {
    fn name() -> &'static str;
    fn all_labels() -> &'static [&'static str];
    fn label(&self) -> &'static str;

    fn initialize_gauges(opts: Opts) -> Result<IntGaugeVec> {
        let gauges = IntGaugeVec::new(opts, &["request"])?;
        for label in Self::all_labels() {
            gauges.with_label_values(&[label]).set(0);
        }

        Ok(gauges)
    }
}

/// A trait abstracting a labeled subsystem, that is a specific use of an HTTP
/// client where the HTTP requests should not be differenciated.
pub trait UnlabeledSubsystem {
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

        impl LabeledSubsystem for $name {
            fn name() -> &'static str {
                $n
            }

            fn all_labels() -> &'static [&'static str] {
                const ALL: &[&str] = &[$($label),*];
                &ALL
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

        impl UnlabeledSubsystem for $name {
            fn name() -> &'static str {
                $n
            }
        }
    };
}

subsystem! {
    /// A label for `web3` HTTP request. This subsystem is used by the HTTP
    /// transport implementation.
    pub enum Web3Label as "web3" {
        Call => "call",
        EstimateGas => "estimate_gas",
        Other => "other",
    }
}

subsystem! {
    /// A label for Kraken HTTP API request. This subsystem is used by the
    /// Kraken API implementation.
    pub enum KrakenLabel as "kraken" {
        Assets => "assets",
        AssetPairs => "asset_pairs",
        TickerInfos => "ticker_infos",
    }
}

subsystem! {
    /// A label for Gnosis Safe gas station HTTP API request. This subsystem is
    /// used by the gas station client implementation.
    pub struct GasStationLabel as "gas_station";
}
