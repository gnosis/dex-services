// Mockall triggers this warning for every mocked trait. This is fixed in Mockall master but not
// released.
#![cfg_attr(test, allow(clippy::unused_unit))]

#[macro_use]
pub mod macros;

pub mod bigint_u256;
pub mod contracts;
pub mod driver;
pub mod economic_viability;
pub mod gas_price;
pub mod health;
pub mod history;
pub mod http;
pub mod http_server;
pub mod logging;
pub mod metrics;
pub mod models;
pub mod orderbook;
pub mod price_estimation;
pub mod price_finding;
pub mod serialization;
pub mod solution_submission;
pub mod time;
pub mod token_info;
pub mod transport;
pub mod util;
