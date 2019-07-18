use failure::Error;
use slog::Logger;
use std::str::FromStr;
use std::sync::Arc;

use driver::models::PendingFlux;

use graph::bigdecimal::BigDecimal;
use graph::components::ethereum::EthereumBlock;
use graph::components::store::{EntityOperation, EntityKey};
use graph::data::store::{Entity};

use web3::types::{Log, Transaction, U256};

use super::EventHandler;
use crate::*;

#[derive(Debug, Clone)]
pub struct DepositHandler {}

impl EventHandler for DepositHandler {
    fn process_event(
        &self,
        logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>
    ) -> Result<Vec<EntityOperation>, Error> {
        info!(logger, "Processing Deposit");
        let flux = from_log(log);
        Ok(to_entity_operation(flux))
    }
}

fn read_u8(bytes: &mut Vec<u8>) -> u8 {
    let mut chunk: [u8; 1] = Default::default();
    chunk.copy_from_slice(bytes.split_off(1).as_slice());
    u8::from_be_bytes(chunk)
}

fn read_u16(bytes: &mut Vec<u8>) -> u16 {
    let mut chunk: [u8; 2] = Default::default();
    chunk.copy_from_slice(bytes.split_off(2).as_slice());
    u16::from_be_bytes(chunk)
}

fn read_u128(bytes: &mut Vec<u8>) -> u128 {
    let mut chunk: [u8; 16] = Default::default();
    chunk.copy_from_slice(bytes.split_off(16).as_slice());
    u128::from_be_bytes(chunk)
}

fn read_u256(bytes: &mut Vec<u8>) -> U256 {
    U256:: from_big_endian(bytes.split_off(32).as_slice())
}

fn from_log(log: Arc<Log>) -> PendingFlux {
    let mut bytes: Vec<u8> = log.data.0.clone();
    PendingFlux {
        account_id: read_u16(&mut bytes),
        token_id: read_u8(&mut bytes),
        amount: read_u128(&mut bytes),
        slot: read_u256(&mut bytes).low_u32(),
        slot_index: read_u16(&mut bytes) as u32,
    }
}

fn to_entity_operation(flux: PendingFlux) -> Vec<EntityOperation> {
    let key = EntityKey {
        subgraph_id: SUBGRAPH_ID.clone(),
        entity_type: "Deposit".to_string(),
        entity_id: "1".to_string(),
    };
    let mut data = Entity::new();
    data.set("accountId", flux.account_id as u64);
    data.set("tokenId", flux.token_id as u64);
    data.set("amount", BigDecimal::from_str(&flux.amount.to_string()).unwrap());
    data.set("slot", flux.slot as u64);
    data.set("slotIndex", flux.slot_index as u64);
    vec![
        EntityOperation::Set {key, data}
    ]
}