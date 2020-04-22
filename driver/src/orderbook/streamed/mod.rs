#![allow(dead_code)]

mod balance;
mod block_timestamp;
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
