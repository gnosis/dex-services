use std::convert::TryInto;
use graph::bigdecimal::BigDecimal;
use graph::data::store::{Entity, Value};
use std::convert::TryFrom;
use std::str::FromStr;
use web3::types::{U256, H256};

pub trait PopFromLogData {
    fn pop_from_log_data(bytes: &mut Vec<u8>) -> Self;
}

impl PopFromLogData for u8 {
    fn pop_from_log_data(bytes: &mut Vec<u8>) -> Self {
        U256::pop_from_log_data(bytes).as_u32().try_into().unwrap()
    }
}

impl PopFromLogData for u16 {
    fn pop_from_log_data(bytes: &mut Vec<u8>) -> Self {
        U256::pop_from_log_data(bytes).as_u32().try_into().unwrap()
    }
}

impl PopFromLogData for u128 {
    fn pop_from_log_data(bytes: &mut Vec<u8>) -> Self {
        U256::pop_from_log_data(bytes).to_string().parse().unwrap()
    }
}

impl PopFromLogData for U256 {
    fn pop_from_log_data(bytes: &mut Vec<u8>) -> Self {
        U256::from_big_endian(
            bytes.drain(0..32).collect::<Vec<u8>>().as_slice()
        )
    }
}

impl PopFromLogData for H256 {
    fn pop_from_log_data(bytes: &mut Vec<u8>) -> Self {
        H256::from(
            bytes.drain(0..32).collect::<Vec<u8>>().as_slice()
        )
    }
}

pub trait ToValue {
    fn to_value(&self) -> Value;
}

impl ToValue for u8 {
    fn to_value(&self) -> Value {
        i32::from(*self).into()
    }
}

impl ToValue for u16 {
    fn to_value(&self) -> Value {
        i32::from(*self).into()
    }
}

impl ToValue for u32 {
    fn to_value(&self) -> Value {
        u128::from(*self).to_value()
    }
}

impl ToValue for u128 {
    fn to_value(&self) -> Value {
        BigDecimal::from_str(&self.to_string()).unwrap().into()
    }
}

impl ToValue for U256 {
    fn to_value(&self) -> Value {
        BigDecimal::from_str(&self.to_string()).unwrap().into()
    }
}

impl ToValue for H256 {
    fn to_value(&self) -> Value {
        format!("{:x}", self).into()
    }
}

impl ToValue for Vec<u128> {
    fn to_value(&self) -> Value {
        self.iter()
            .map(|balance| balance.to_value())
            .collect::<Vec<Value>>()
            .into()
    }
}

pub trait EntityParsing {
    fn from_entity(entity: &Entity, field: &str) -> Self;
}

impl EntityParsing for u8 {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        u8::try_from(entity
            .get(field)
            .and_then(|value| value.clone().as_int())
            .unwrap_or_else(|| panic!("Couldn't get field {} as uint", field))
        ).unwrap_or_else(|_| panic!("Couldn't cast {} from i32 to u8", field))
    }
}

impl EntityParsing for u16 {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        u16::try_from(entity
            .get(field)
            .and_then(|value| value.clone().as_int())
            .unwrap_or_else(|| panic!("Couldn't get field {} as uint", field))
        ).unwrap_or_else(|_| panic!("Couldn't cast {} from i32 to u16", field))
    }
}

impl EntityParsing for u128 {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        u128::from_str(&entity
            .get(field)
            .and_then(|value| value.clone().as_big_decimal())
            .map(|decimal| decimal.to_string())
            .unwrap_or_else(|| panic!("Couldn't get field {} as big decimal", field))
        ).unwrap_or_else(|_| panic!("Couldn't cast {} from string to u128", field))
    }
}

impl EntityParsing for U256 {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        U256::from_str(&entity
            .get(field)
            .and_then(|value| value.clone().as_big_decimal())
            .map(|decimal| decimal.to_string())
            .unwrap_or_else(|| panic!("Couldn't get field {} as big decimal", field))
        ).unwrap_or_else(|_| panic!("Couldn't cast {} from string to U256", field))
    }
}

impl EntityParsing for H256 {
    fn from_entity(entity: &Entity, field: &str) -> Self {
        H256::from_str(&entity
            .get(field)
            .and_then(|value| value.clone().as_string())
            .unwrap_or_else(|| panic!("Couldn't get field {} as string", field))
        ).unwrap_or_else(|_| panic!("Couldn't cast {} from string to H256", field))
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
                            .unwrap_or_else(|| panic!("Couldn't convert value {} to big decimal", &value))
                    ).unwrap_or_else(|_| panic!("Couldn't parse value {} to u128", &value)))
                .collect::<Vec<u128>>()
            ).unwrap_or_else(|| panic!("Couldn't get field {} as list", field))
    }
}