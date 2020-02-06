// NOTE: The order in which these two crates get linked seems to matter (usure
//   why). And when we remove `extern crate` statements and let cargo decide the
//   order it leads to a linking error. So for now, until we figure out exactly
//   why this is happening lets keep this these two `extern crate` statements so
//   we successfully link.
extern crate ethereum_tx_sign;
extern crate lazy_static;
extern crate web3;

pub mod contracts;
pub mod driver;
pub mod error;
pub mod logging;
pub mod metrics;
pub mod orderbook;
pub mod price_finding;
pub mod solution_submission;
pub mod transport;
pub mod util;
