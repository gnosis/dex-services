#![allow(dead_code)]

mod balance;
mod order;
mod state;

use ethcontract::Address;
use ethcontract::U256;

type UserId = Address;
type TokenAddress = Address;
type OrderId = u16;
type TokenId = u16;
type BatchId = u32;
