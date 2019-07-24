use std::convert::TryInto;
use graph::bigdecimal::BigDecimal;
use graph::data::store::{Entity, Value};
use std::convert::TryFrom;
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

pub trait EntityParsing {
    fn from_entity(entity: &Entity, field: &str) -> Self;
}

impl EntityParsing for u8 {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        u8::try_from(entity
            .get(field)
            .and_then(|value| value.clone().as_int())
            .expect(&format!("Couldn't get field {} as uint", field))
        ).expect(&format!("Couldn't cast {} from i32 to u8", field))
    }
}

impl EntityParsing for u16 {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        u16::try_from(entity
            .get(field)
            .and_then(|value| value.clone().as_int())
            .expect(&format!("Couldn't get field {} as uint", field))
        ).expect(&format!("Couldn't cast {} from i32 to u16", field))
    }
}

impl EntityParsing for u128 {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        u128::from_str(&entity
            .get(field)
            .and_then(|value| value.clone().as_big_decimal())
            .map(|decimal| decimal.to_string())
            .expect(&format!("Couldn't get field {} as big decimal", field))
        ).expect(&format!("Couldn't cast {} from string to u128", field))
    }
}

impl EntityParsing for U256 {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        U256::from_str(&entity
            .get(field)
            .and_then(|value| value.clone().as_big_decimal())
            .map(|decimal| decimal.to_string())
            .expect(&format!("Couldn't get field {} as big decimal", field))
        ).expect(&format!("Couldn't cast {} from string to U256", field))
    }
}

impl EntityParsing for H256 {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        H256::from_str(&entity
            .get(field)
            .and_then(|value| value.clone().as_string())
            .expect(&format!("Couldn't get field {} as string", field))
        ).expect(&format!("Couldn't cast {} from string to H256", field))
    }
}

impl EntityParsing for Vec<u128> {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        entity
            .get(field)
            .and_then(|value| value.clone().as_list())
            .map(|list| list
                .into_iter()
                .map(|value| u128::from_str(
                        &value.clone()
                            .as_big_decimal()
                            .map(|decimal| decimal.to_string())
                            .expect(&format!("Couldn't convert value {} to big decimal", &value))
                    ).unwrap_or_else(|_| panic!("Couldn't parse value {} to u128", &value)))
                .collect::<Vec<u128>>()
            ).unwrap_or_else(|| panic!("Couldn't get field {} as list", field))
    }
}