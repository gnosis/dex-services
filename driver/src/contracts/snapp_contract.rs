#![allow(clippy::ptr_arg)] // required for automock

use log::{debug, info};
#[cfg(test)]
use mockall::automock;
use std::env;
use web3::api::Web3;
use web3::futures::Future;
use web3::transports::{EventLoopHandle, Http};
use web3::types::{BlockId, H160, H256, U128, U256};
use web3::Transport;

use crate::contracts;
use crate::error::DriverError;
use crate::transport::LoggingTransport;
use crate::util::FutureWaitExt;

type Result<T> = std::result::Result<T, DriverError>;

include!(concat!(env!("OUT_DIR"), "/snapp_auction.rs"));

pub struct SnappContractImpl<T>
where
    T: Transport,
{
    web3: Web3<T>,
    _event_loop: EventLoopHandle,
    instance: SnappAuction,
}

impl SnappContractImpl<LoggingTransport<Http>> {
    pub fn new(ethereum_node_url: String, network_id: u64) -> Result<Self> {
        let (web3, event_loop) = contracts::web3_provider(ethereum_node_url)?;
        let defaults = contracts::method_defaults(network_id)?;

        let mut instance = SnappAuction::deployed(&web3).wait()?;
        *instance.defaults_mut() = defaults;

        Ok(SnappContractImpl {
            web3,
            _event_loop: event_loop,
            instance,
        })
    }
}

impl<T> SnappContractImpl<T>
where
    T: Transport,
{
    pub fn address(&self) -> H160 {
        self.instance.address()
    }

    pub fn account(&self) -> H160 {
        self.instance
            .defaults()
            .from
            .as_ref()
            .map(|from| from.address())
            .unwrap_or_default()
    }
}

#[cfg_attr(test, automock)]
pub trait SnappContract {
    // General Blockchain interface
    fn get_current_block_timestamp(&self) -> Result<U256>;

    // Top level smart contract methods
    fn get_current_state_root(&self) -> Result<H256>;
    fn get_current_deposit_slot(&self) -> Result<U256>;
    fn get_current_withdraw_slot(&self) -> Result<U256>;
    fn get_current_auction_slot(&self) -> Result<U256>;
    fn calculate_order_hash(&self, slot: U256, standing_order_index: Vec<U128>) -> Result<H256>;

    // Deposit Slots
    fn creation_timestamp_for_deposit_slot(&self, slot: U256) -> Result<U256>;
    fn deposit_hash_for_slot(&self, slot: U256) -> Result<H256>;
    fn has_deposit_slot_been_applied(&self, slot: U256) -> Result<bool>;

    // Withdraw Slots
    fn creation_timestamp_for_withdraw_slot(&self, slot: U256) -> Result<U256>;
    fn withdraw_hash_for_slot(&self, slot: U256) -> Result<H256>;
    fn has_withdraw_slot_been_applied(&self, slot: U256) -> Result<bool>;

    // Auction Slots
    fn creation_timestamp_for_auction_slot(&self, slot: U256) -> Result<U256>;
    fn order_hash_for_slot(&self, slot: U256) -> Result<H256>;
    fn has_auction_slot_been_applied(&self, slot: U256) -> Result<bool>;

    // Write methods
    fn apply_deposits(
        &self,
        slot: U256,
        prev_state: H256,
        new_state: H256,
        deposit_hash: H256,
    ) -> Result<()>;
    fn apply_withdraws(
        &self,
        slot: U256,
        merkle_root: H256,
        prev_state: H256,
        new_state: H256,
        withdraw_hash: H256,
    ) -> Result<()>;
    fn apply_auction(
        &self,
        slot: U256,
        prev_state: H256,
        new_state: H256,
        prices_and_volumes: Vec<u8>,
    ) -> Result<()>;
    fn auction_solution_bid(
        &self,
        slot: U256,
        prev_state: H256,
        new_state: H256,
        order_hash: H256,
        standing_order_index: Vec<U128>,
        objective_value: U256,
    ) -> Result<()>;
}

impl<T> SnappContract for SnappContractImpl<T>
where
    T: Transport,
{
    fn get_current_block_timestamp(&self) -> Result<U256> {
        self.web3
            .eth()
            .block_number()
            .wait()
            .and_then(|block_number| {
                self.web3
                    .eth()
                    .block(BlockId::from(block_number.as_u64()))
                    .wait()
            })
            .map_err(DriverError::from)
            .and_then(|block_option| match block_option {
                Some(block) => Ok(block.timestamp),
                None => Err(DriverError::from("Current block not found")),
            })
    }

    fn get_current_state_root(&self) -> Result<H256> {
        Ok(self.instance.get_current_state_root().call().wait()?.into())
    }

    fn get_current_deposit_slot(&self) -> Result<U256> {
        Ok(self.instance.get_current_deposit_index().call().wait()?)
    }

    fn get_current_withdraw_slot(&self) -> Result<U256> {
        Ok(self.instance.get_current_withdraw_index().call().wait()?)
    }

    fn get_current_auction_slot(&self) -> Result<U256> {
        Ok(self.instance.auction_index().call().wait()?)
    }

    fn calculate_order_hash(&self, slot: U256, standing_order_index: Vec<U128>) -> Result<H256> {
        Ok(self
            .instance
            .calculate_order_hash(slot, standing_order_index)
            .gas(5_000_000.into())
            .call()
            .wait()?
            .into())
    }

    fn creation_timestamp_for_deposit_slot(&self, slot: U256) -> Result<U256> {
        Ok(self
            .instance
            .get_deposit_creation_timestamp(slot)
            .call()
            .wait()?)
    }

    fn deposit_hash_for_slot(&self, slot: U256) -> Result<H256> {
        Ok(self.instance.get_deposit_hash(slot).call().wait()?.into())
    }

    fn has_deposit_slot_been_applied(&self, slot: U256) -> Result<bool> {
        Ok(self.instance.has_deposit_been_applied(slot).call().wait()?)
    }

    fn creation_timestamp_for_withdraw_slot(&self, slot: U256) -> Result<U256> {
        Ok(self
            .instance
            .get_withdraw_creation_timestamp(slot)
            .call()
            .wait()?)
    }

    fn withdraw_hash_for_slot(&self, slot: U256) -> Result<H256> {
        Ok(self.instance.get_withdraw_hash(slot).call().wait()?.into())
    }

    fn has_withdraw_slot_been_applied(&self, slot: U256) -> Result<bool> {
        Ok(self
            .instance
            .has_withdraw_been_applied(slot)
            .call()
            .wait()?)
    }

    fn creation_timestamp_for_auction_slot(&self, slot: U256) -> Result<U256> {
        Ok(self
            .instance
            .get_auction_creation_timestamp(slot)
            .call()
            .wait()?)
    }

    fn order_hash_for_slot(&self, slot: U256) -> Result<H256> {
        Ok(self.instance.get_order_hash(slot).call().wait()?.into())
    }

    fn has_auction_slot_been_applied(&self, slot: U256) -> Result<bool> {
        Ok(self.instance.has_auction_been_applied(slot).call().wait()?)
    }

    fn apply_deposits(
        &self,
        slot: U256,
        prev_state: H256,
        new_state: H256,
        deposit_hash: H256,
    ) -> Result<()> {
        self.instance
            .apply_deposits(
                slot,
                prev_state.to_fixed_bytes(),
                new_state.to_fixed_bytes(),
                deposit_hash.to_fixed_bytes(),
            )
            .send()
            .wait()?;
        Ok(())
    }

    fn apply_withdraws(
        &self,
        slot: U256,
        merkle_root: H256,
        prev_state: H256,
        new_state: H256,
        withdraw_hash: H256,
    ) -> Result<()> {
        // SENDING ACCOUNT MUST BE CONTRACT OWNER
        self.instance
            .apply_withdrawals(
                slot,
                merkle_root.to_fixed_bytes(),
                prev_state.to_fixed_bytes(),
                new_state.to_fixed_bytes(),
                withdraw_hash.to_fixed_bytes(),
            )
            .gas(1_000_000.into())
            .send()
            .wait()?;
        Ok(())
    }

    fn apply_auction(
        &self,
        slot: U256,
        prev_state: H256,
        new_state: H256,
        prices_and_volumes: Vec<u8>,
    ) -> Result<()> {
        debug!(
            "Applying Auction with result bytes: {:?}",
            &prices_and_volumes
        );

        self.instance
            .apply_auction(
                slot,
                prev_state.to_fixed_bytes(),
                new_state.to_fixed_bytes(),
                prices_and_volumes,
            )
            .gas(5_000_000.into())
            .send()
            .wait()?;
        Ok(())
    }

    fn auction_solution_bid(
        &self,
        slot: U256,
        prev_state: H256,
        new_state: H256,
        order_hash: H256,
        standing_order_index: Vec<U128>,
        objective_value: U256,
    ) -> Result<()> {
        info!("objective value: {:?}", &objective_value);

        self.instance
            .auction_solution_bid(
                slot,
                prev_state.to_fixed_bytes(),
                order_hash.to_fixed_bytes(),
                standing_order_index,
                new_state.to_fixed_bytes(),
                objective_value,
            )
            .gas(5_000_000.into())
            .send()
            .wait()?;
        Ok(())
    }
}
