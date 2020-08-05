// TODO(nlordell): Remove this lint once we release a new `ethcontract` version
// that does not trigger this lint.
#![allow(unused_braces)]

#[cfg(feature = "bin")]
pub mod paths;

include!(concat!(env!("OUT_DIR"), "/BatchExchange.rs"));
include!(concat!(env!("OUT_DIR"), "/BatchExchangeViewer.rs"));
