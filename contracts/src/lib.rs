#[cfg(feature = "bin")]
pub mod paths;

include!(concat!(env!("OUT_DIR"), "/BatchExchange.rs"));
include!(concat!(env!("OUT_DIR"), "/BatchExchangeViewer.rs"));
include!(concat!(env!("OUT_DIR"), "/IdToAddressBiMap.rs"));
include!(concat!(env!("OUT_DIR"), "/IterableAppendOnlySet.rs"));
include!(concat!(env!("OUT_DIR"), "/TokenOWL.rs"));
include!(concat!(env!("OUT_DIR"), "/TokenOWLProxy.rs"));
