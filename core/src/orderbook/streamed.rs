#![allow(dead_code)]

mod balance;
mod bigint_u256;
mod block_timestamp_reading;
mod order;
mod orderbook;
mod state;
mod updating_orderbook;

use ethcontract::Address;
use ethcontract::U256;

type UserId = Address;
type TokenAddress = Address;
type OrderId = u16;
type TokenId = u16;
type BatchId = u32;

pub use block_timestamp_reading::BlockTimestampReading;
pub use updating_orderbook::UpdatingOrderbook as Orderbook;
