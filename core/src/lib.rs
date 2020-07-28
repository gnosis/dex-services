// Mockall triggers this warning for every mocked trait. This is fixed in Mockall master but not
// released.
#![cfg_attr(test, allow(clippy::unused_unit))]
// Coverage build on nightly is failing because of this
#![allow(unused_braces)]

#[macro_use]
pub mod macros;

pub mod bigint_u256;
pub mod contracts;
pub mod driver;
pub mod gas_station;
pub mod history;
pub mod http;
pub mod logging;
pub mod metrics;
pub mod models;
pub mod orderbook;
pub mod price_estimation;
pub mod price_finding;
pub mod solution_submission;
pub mod token_info;
pub mod transport;
pub mod util;
