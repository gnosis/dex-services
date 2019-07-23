use std::convert::TryInto;
use graph::bigdecimal::BigDecimal;
use graph::data::store::Value;
use std::str::FromStr;
use web3::types::{U256, H256};

pub fn pop_u8_from_log_data(bytes: &mut Vec<u8>) -> u8 {
    pop_u256_from_log_data(bytes).as_u32().try_into().unwrap()
}

pub fn pop_u16_from_log_data(bytes: &mut Vec<u8>) -> u16 {
    pop_u256_from_log_data(bytes).as_u32().try_into().unwrap()
}

pub fn pop_u128_from_log_data(bytes: &mut Vec<u8>) -> u128 {
    pop_u256_from_log_data(bytes).to_string().parse().unwrap()
}

pub fn pop_u256_from_log_data(bytes: &mut Vec<u8>) -> U256 {
    U256::from_big_endian(
        bytes.drain(0..32).collect::<Vec<u8>>().as_slice()
    )
}

pub fn pop_h256_from_log_data(bytes: &mut Vec<u8>) -> H256 {
    H256::from(
        bytes.drain(0..32).collect::<Vec<u8>>().as_slice()
    )
}

pub fn to_value<T: ToString>(value: &T) -> Value {
    Value::from(
        BigDecimal::from_str(&value.to_string()).unwrap()
    )
}