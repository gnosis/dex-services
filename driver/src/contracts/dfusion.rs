#[cfg(test)]
extern crate mock_it;

use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::types::{Address, BlockId, H256, U128, U256};

use crate::error::DriverError;

use std::env;
use std::fs;

type Result<T> = std::result::Result<T, DriverError>;

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

#[allow(dead_code)] // event_loop needs to be retained to keep web3 connection open
pub struct SnappContractImpl {
    contract: Contract<web3::transports::Http>,
    web3: web3::Web3<web3::transports::Http>,
    event_loop: web3::transports::EventLoopHandle,
}

impl SnappContractImpl {
    pub fn new() -> Result<Self> {
        let (event_loop, transport) =
            web3::transports::Http::new(&(env::var("ETHEREUM_NODE_URL")?))?;
        let web3 = web3::Web3::new(transport);

        let contents = fs::read_to_string("dex-contracts/build/contracts/SnappAuction.json")?;
        let snapp_base: serde_json::Value = serde_json::from_str(&contents)?;
        let snapp_base_abi: String = snapp_base
            .get("abi")
            .ok_or("No ABI for contract")?
            .to_string();

        let snapp_address = hex::decode(&(env::var("SNAPP_CONTRACT_ADDRESS")?)[2..])?;
        let address: Address = Address::from(&snapp_address[..]);
        let contract = Contract::from_json(web3.eth(), address, snapp_base_abi.as_bytes())?;
        Ok(SnappContractImpl {
            contract,
            web3,
            event_loop,
        })
    }

    fn account_with_sufficient_balance(&self) -> Option<Address> {
        let accounts: Vec<Address> = self.web3.eth().accounts().wait().ok()?;
        accounts
            .into_iter()
            .find(|&acc| match self.web3.eth().balance(acc, None).wait() {
                Ok(balance) => !balance.is_zero(),
                Err(_) => false,
            })
    }
}

impl SnappContract for SnappContractImpl {
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
            .and_then(|block_option| match block_option {
                Some(block) => Ok(block.timestamp),
                None => Err(web3::Error::from("Current block not found")),
            })
            .map_err(DriverError::from)
    }

    fn get_current_state_root(&self) -> Result<H256> {
        self.contract
            .query("getCurrentStateRoot", (), None, Options::default(), None)
            .wait()
            .map_err(DriverError::from)
    }

    fn get_current_deposit_slot(&self) -> Result<U256> {
        self.contract
            .query("getCurrentDepositIndex", (), None, Options::default(), None)
            .wait()
            .map_err(DriverError::from)
    }

    fn get_current_withdraw_slot(&self) -> Result<U256> {
        self.contract
            .query(
                "getCurrentWithdrawIndex",
                (),
                None,
                Options::default(),
                None,
            )
            .wait()
            .map_err(DriverError::from)
    }

    fn get_current_auction_slot(&self) -> Result<U256> {
        self.contract
            .query("auctionIndex", (), None, Options::default(), None)
            .wait()
            .map_err(DriverError::from)
    }

    fn calculate_order_hash(&self, slot: U256, standing_order_index: Vec<U128>) -> Result<H256> {
        self.contract
            .query(
                "calculateOrderHash",
                (slot, standing_order_index),
                None,
                Options::default(),
                None,
            )
            .wait()
            .map_err(DriverError::from)
    }

    fn creation_timestamp_for_deposit_slot(&self, slot: U256) -> Result<U256> {
        self.contract
            .query(
                "getDepositCreationTimestamp",
                slot,
                None,
                Options::default(),
                None,
            )
            .wait()
            .map_err(DriverError::from)
    }

    fn deposit_hash_for_slot(&self, slot: U256) -> Result<H256> {
        self.contract
            .query("getDepositHash", slot, None, Options::default(), None)
            .wait()
            .map_err(DriverError::from)
    }

    fn has_deposit_slot_been_applied(&self, slot: U256) -> Result<bool> {
        self.contract
            .query(
                "hasDepositBeenApplied",
                slot,
                None,
                Options::default(),
                None,
            )
            .wait()
            .map_err(DriverError::from)
    }

    fn creation_timestamp_for_withdraw_slot(&self, slot: U256) -> Result<U256> {
        self.contract
            .query(
                "getWithdrawCreationTimestamp",
                slot,
                None,
                Options::default(),
                None,
            )
            .wait()
            .map_err(DriverError::from)
    }

    fn withdraw_hash_for_slot(&self, slot: U256) -> Result<H256> {
        self.contract
            .query("getWithdrawHash", slot, None, Options::default(), None)
            .wait()
            .map_err(DriverError::from)
    }

    fn has_withdraw_slot_been_applied(&self, slot: U256) -> Result<bool> {
        self.contract
            .query(
                "hasWithdrawBeenApplied",
                slot,
                None,
                Options::default(),
                None,
            )
            .wait()
            .map_err(DriverError::from)
    }

    fn creation_timestamp_for_auction_slot(&self, slot: U256) -> Result<U256> {
        self.contract
            .query(
                "getAuctionCreationTimestamp",
                slot,
                None,
                Options::default(),
                None,
            )
            .wait()
            .map_err(DriverError::from)
    }

    fn order_hash_for_slot(&self, slot: U256) -> Result<H256> {
        self.contract
            .query("getOrderHash", slot, None, Options::default(), None)
            .wait()
            .map_err(DriverError::from)
    }

    fn has_auction_slot_been_applied(&self, slot: U256) -> Result<bool> {
        self.contract
            .query(
                "hasAuctionBeenApplied",
                slot,
                None,
                Options::default(),
                None,
            )
            .wait()
            .map_err(DriverError::from)
    }

    fn apply_deposits(
        &self,
        slot: U256,
        prev_state: H256,
        new_state: H256,
        deposit_hash: H256,
    ) -> Result<()> {
        let account = self
            .account_with_sufficient_balance()
            .ok_or("Not enough balance to send Txs")?;
        self.contract
            .call(
                "applyDeposits",
                (slot, prev_state, new_state, deposit_hash),
                account,
                Options::default(),
            )
            .wait()
            .map_err(DriverError::from)
            .map(|_| ())
    }

    fn apply_withdraws(
        &self,
        slot: U256,
        merkle_root: H256,
        prev_state: H256,
        new_state: H256,
        withdraw_hash: H256,
    ) -> Result<()> {
        // HERE WE NEED TO BE SURE THAT THE SENDING ACCOUNT IS THE OWNER
        let account = self
            .account_with_sufficient_balance()
            .ok_or("Not enough balance to send Txs")?;
        self.contract
            .call(
                "applyWithdrawals",
                (slot, merkle_root, prev_state, new_state, withdraw_hash),
                account,
                Options::with(|mut opt| {
                    // usual gas estimate is not working
                    opt.gas_price = Some(25.into());
                    opt.gas = Some(1_000_000.into());
                }),
            )
            .wait()
            .map_err(DriverError::from)
            .map(|_| ())
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
        let account = self
            .account_with_sufficient_balance()
            .ok_or("Not enough balance to send Txs")?;

        let mut options = Options::default();
        options.gas = Some(U256::from(5_000_000));
        self.contract
            .call(
                "applyAuction",
                (slot, prev_state, new_state, prices_and_volumes),
                account,
                options,
            )
            .wait()
            .map_err(DriverError::from)
            .map(|_| ())
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
        let account = self
            .account_with_sufficient_balance()
            .ok_or("Not enough balance to send Txs")?;

        let mut options = Options::default();
        options.gas = Some(U256::from(5_000_000));
        self.contract
            .call(
                "auctionSolutionBid",
                (
                    slot,
                    prev_state,
                    order_hash,
                    standing_order_index,
                    new_state,
                    objective_value,
                ),
                account,
                options,
            )
            .wait()
            .map_err(DriverError::from)
            .map(|_| ())
    }

}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use mock_it::Matcher;
    use mock_it::Matcher::*;
    use mock_it::Mock;

    type ApplyDepositArguments = (U256, Matcher<H256>, Matcher<H256>, Matcher<H256>);
    type ApplyWithdrawArguments = (
        U256,
        Matcher<H256>,
        Matcher<H256>,
        Matcher<H256>,
        Matcher<H256>,
    );
    type ApplyAuctionArguments = (U256, Matcher<H256>, Matcher<H256>, Matcher<Vec<u8>>);
    type ApplySolutionArguments = (
        U256,
        Matcher<H256>,
        Matcher<H256>,
        Matcher<H256>,
        Matcher<Vec<U128>>,
        U256,
    );

    #[derive(Clone)]
    pub struct SnappContractMock {
        pub get_current_block_timestamp: Mock<(), Result<U256>>,
        pub get_current_state_root: Mock<(), Result<H256>>,
        pub get_current_deposit_slot: Mock<(), Result<U256>>,
        pub get_current_withdraw_slot: Mock<(), Result<U256>>,
        pub get_current_auction_slot: Mock<(), Result<U256>>,
        pub creation_timestamp_for_deposit_slot: Mock<(U256), Result<U256>>,
        pub deposit_hash_for_slot: Mock<U256, Result<H256>>,
        pub has_deposit_slot_been_applied: Mock<U256, Result<bool>>,
        pub creation_timestamp_for_withdraw_slot: Mock<U256, Result<U256>>,
        pub withdraw_hash_for_slot: Mock<U256, Result<H256>>,
        pub has_withdraw_slot_been_applied: Mock<U256, Result<bool>>,
        pub creation_timestamp_for_auction_slot: Mock<U256, Result<U256>>,
        pub order_hash_for_slot: Mock<U256, Result<H256>>,
        pub has_auction_slot_been_applied: Mock<U256, Result<bool>>,
        pub apply_deposits: Mock<ApplyDepositArguments, Result<()>>,
        pub apply_withdraws: Mock<ApplyWithdrawArguments, Result<()>>,
        pub apply_auction: Mock<ApplyAuctionArguments, Result<()>>,
        pub auction_solution_bid: Mock<ApplySolutionArguments, Result<()>>,
        pub calculate_order_hash: Mock<(U256, Matcher<Vec<U128>>), Result<H256>>,
    }

    impl Default for SnappContractMock {
        fn default() -> SnappContractMock {
            SnappContractMock {
                get_current_block_timestamp: Mock::new(Err(DriverError::new(
                    "Unexpected call to get_current_block_timestamp",
                    ErrorKind::Unknown,
                ))),
                get_current_state_root: Mock::new(Err(DriverError::new(
                    "Unexpected call to get_current_state_root",
                    ErrorKind::Unknown,
                ))),
                get_current_deposit_slot: Mock::new(Err(DriverError::new(
                    "Unexpected call to get_current_deposit_slot",
                    ErrorKind::Unknown,
                ))),
                get_current_withdraw_slot: Mock::new(Err(DriverError::new(
                    "Unexpected call to get_current_withdraw_slot",
                    ErrorKind::Unknown,
                ))),
                get_current_auction_slot: Mock::new(Err(DriverError::new(
                    "Unexpected call to get_current_auction_slot",
                    ErrorKind::Unknown,
                ))),
                creation_timestamp_for_deposit_slot: Mock::new(Err(DriverError::new(
                    "Unexpected call to creation_timestamp_for_deposit_slot",
                    ErrorKind::Unknown,
                ))),
                deposit_hash_for_slot: Mock::new(Err(DriverError::new(
                    "Unexpected call to deposit_hash_for_slot",
                    ErrorKind::Unknown,
                ))),
                has_deposit_slot_been_applied: Mock::new(Err(DriverError::new(
                    "Unexpected call to has_deposit_slot_been_applied",
                    ErrorKind::Unknown,
                ))),
                creation_timestamp_for_withdraw_slot: Mock::new(Err(DriverError::new(
                    "Unexpected call to creation_timestamp_for_withdraw_slot",
                    ErrorKind::Unknown,
                ))),
                withdraw_hash_for_slot: Mock::new(Err(DriverError::new(
                    "Unexpected call to withdraw_hash_for_slot",
                    ErrorKind::Unknown,
                ))),
                has_withdraw_slot_been_applied: Mock::new(Err(DriverError::new(
                    "Unexpected call to has_withdraw_slot_been_applied",
                    ErrorKind::Unknown,
                ))),
                creation_timestamp_for_auction_slot: Mock::new(Err(DriverError::new(
                    "Unexpected call to creation_timestamp_for_auction_slot",
                    ErrorKind::Unknown,
                ))),
                order_hash_for_slot: Mock::new(Err(DriverError::new(
                    "Unexpected call to order_hash_for_slot",
                    ErrorKind::Unknown,
                ))),
                has_auction_slot_been_applied: Mock::new(Err(DriverError::new(
                    "Unexpected call to has_auction_slot_been_applied",
                    ErrorKind::Unknown,
                ))),
                apply_deposits: Mock::new(Err(DriverError::new(
                    "Unexpected call to apply_deposits",
                    ErrorKind::Unknown,
                ))),
                apply_withdraws: Mock::new(Err(DriverError::new(
                    "Unexpected call to apply_withdraws",
                    ErrorKind::Unknown,
                ))),
                apply_auction: Mock::new(Err(DriverError::new(
                    "Unexpected call to apply_auctions",
                    ErrorKind::Unknown,
                ))),
                auction_solution_bid: Mock::new(Err(DriverError::new(
                    "Unexpected call to auction_solution_bid",
                    ErrorKind::Unknown,
                ))),
                calculate_order_hash: Mock::new(Err(DriverError::new(
                    "Unexpected call to calculate_order_hash",
                    ErrorKind::Unknown,
                ))),
            }
        }
    }

    impl SnappContract for SnappContractMock {
        fn get_current_block_timestamp(&self) -> Result<U256> {
            self.get_current_block_timestamp.called(())
        }
        fn get_current_state_root(&self) -> Result<H256> {
            self.get_current_state_root.called(())
        }
        fn get_current_deposit_slot(&self) -> Result<U256> {
            self.get_current_deposit_slot.called(())
        }
        fn get_current_withdraw_slot(&self) -> Result<U256> {
            self.get_current_withdraw_slot.called(())
        }
        fn get_current_auction_slot(&self) -> Result<U256> {
            self.get_current_auction_slot.called(())
        }
        fn creation_timestamp_for_deposit_slot(&self, slot: U256) -> Result<U256> {
            self.creation_timestamp_for_deposit_slot.called(slot)
        }
        fn deposit_hash_for_slot(&self, slot: U256) -> Result<H256> {
            self.deposit_hash_for_slot.called(slot)
        }
        fn has_deposit_slot_been_applied(&self, slot: U256) -> Result<bool> {
            self.has_deposit_slot_been_applied.called(slot)
        }
        fn creation_timestamp_for_withdraw_slot(&self, slot: U256) -> Result<U256> {
            self.creation_timestamp_for_withdraw_slot.called(slot)
        }
        fn withdraw_hash_for_slot(&self, slot: U256) -> Result<H256> {
            self.withdraw_hash_for_slot.called(slot)
        }
        fn has_withdraw_slot_been_applied(&self, slot: U256) -> Result<bool> {
            self.has_withdraw_slot_been_applied.called(slot)
        }
        fn creation_timestamp_for_auction_slot(&self, slot: U256) -> Result<U256> {
            self.creation_timestamp_for_auction_slot.called(slot)
        }
        fn order_hash_for_slot(&self, slot: U256) -> Result<H256> {
            self.order_hash_for_slot.called(slot)
        }
        fn has_auction_slot_been_applied(&self, slot: U256) -> Result<bool> {
            self.has_auction_slot_been_applied.called(slot)
        }
        fn apply_deposits(
            &self,
            slot: U256,
            prev_state: H256,
            new_state: H256,
            deposit_hash: H256,
        ) -> Result<()> {
            self.apply_deposits
                .called((slot, Val(prev_state), Val(new_state), Val(deposit_hash)))
        }
        fn apply_withdraws(
            &self,
            slot: U256,
            merkle_root: H256,
            prev_state: H256,
            new_state: H256,
            withdraw_hash: H256,
        ) -> Result<()> {
            self.apply_withdraws.called((
                slot,
                Val(merkle_root),
                Val(prev_state),
                Val(new_state),
                Val(withdraw_hash),
            ))
        }
        fn apply_auction(
            &self,
            slot: U256,
            prev_state: H256,
            new_state: H256,
            prices_and_volumes: Vec<u8>,
        ) -> Result<()> {
            self.apply_auction.called((
                slot,
                Val(prev_state),
                Val(new_state),
                Val(prices_and_volumes),
            ))
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
            self.auction_solution_bid.called((
                slot,
                Val(prev_state),
                Val(new_state),
                Val(order_hash),
                Val(standing_order_index),
                objective_value,
            ))
        }
        fn calculate_order_hash(
            &self,
            slot: U256,
            standing_order_index: Vec<U128>,
        ) -> Result<H256> {
            self.calculate_order_hash
                .called((slot, Val(standing_order_index)))
        }
    }
}
