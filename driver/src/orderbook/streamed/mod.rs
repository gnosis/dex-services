#![allow(dead_code)]

mod state;

use ethcontract::Address;

type UserId = Address;
type TokenAddress = Address;
type OrderId = u16;
type TokenId = u16;
type BatchId = u32;