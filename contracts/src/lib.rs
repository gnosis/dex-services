// TODO(nlordell): Remove this lint once we release a new `ethcontract` version
// that does not trigger this lint.
#![allow(unused_braces)]

#[cfg(feature = "bin")]
pub mod paths;

include!(concat!(env!("OUT_DIR"), "/BatchExchange.rs"));
include!(concat!(env!("OUT_DIR"), "/BatchExchangeViewer.rs"));
include!(concat!(env!("OUT_DIR"), "/ERC20Mintable.rs"));
include!(concat!(env!("OUT_DIR"), "/IERC20.rs"));
include!(concat!(env!("OUT_DIR"), "/IdToAddressBiMap.rs"));
include!(concat!(env!("OUT_DIR"), "/IterableAppendOnlySet.rs"));
include!(concat!(env!("OUT_DIR"), "/TokenOWL.rs"));
include!(concat!(env!("OUT_DIR"), "/TokenOWLProxy.rs"));
