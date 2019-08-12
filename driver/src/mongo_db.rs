#[cfg(test)]
extern crate mock_it;

use dfusion_core::models;
use dfusion_core::database::{DbInterface, DatabaseError, ErrorKind::*};

use mongodb::{bson, doc};
use mongodb::ordered::OrderedDocument;
use mongodb::db::ThreadedDatabase;
use mongodb::{Client, ThreadedClient};

use web3::types::H256;

#[derive(Clone)]
pub struct MongoDB {
    pub client: Client,
}
impl MongoDB {
    pub fn new(db_host: String, db_port: String) -> Result<MongoDB, DatabaseError> {
        let port = db_port
            .parse::<u16>()
            .map_err(
                |e| DatabaseError::chain(ConfigurationError, "Couldn't parse port", e)
            )?;

        // Connect is being picked up from a trait which isn't in scope (NetworkConnector)
        // https://github.com/intellij-rust/intellij-rust/issues/3654
        let client = Client::connect(&db_host, port)
            .map_err(
                |e| DatabaseError::chain(ConnectionError, "Error connecting client", e)
            )?;
        Ok(MongoDB { client })
    }

    fn get_items_from_query<I: From<mongodb::ordered::OrderedDocument> + std::cmp::Ord>(
        &self,
        query: mongodb::Document,
        collection: &str,
    ) -> Result<Vec<I>, DatabaseError> {
        info!("Querying {}: {}", collection, query);

        let coll = self.client.db(models::DB_NAME).collection(collection);
        let cursor = coll.find(Some(query), None)
            .map_err(
                |e| DatabaseError::chain(ConnectionError, "Failed to find items", e)
            )?;
        let mut docs = vec!();
        for result in cursor {
            let result = result.map_err(
                |e| DatabaseError::chain(ConnectionError, "Cursor Error", e)
            )?;
            docs.push(I::from(result));
        }
        docs.sort();
        Ok(docs)
    }
}

impl DbInterface for MongoDB {
    fn get_current_balances(
        &self,
        current_state_root: &H256,
    ) -> Result<models::AccountState, DatabaseError> {
        let query = doc!{ "stateHash" => format!("{:x}", current_state_root) };
        info!("Querying stateHash: {}", query);

        let coll = self.client.db(models::DB_NAME).collection("accounts");
        let cursor = coll.find(Some(query), None)
            .map_err(
                |e| DatabaseError::chain(ConnectionError, "get_balances failed to find items", e)
            )?;
        let mut docs: Vec<OrderedDocument> = vec!();
        for result in cursor {
            let result = result.map_err(
                |e| DatabaseError::chain(ConnectionError, "get_balances cursor Error", e)
            )?;
            docs.push(result);
        }
        if docs.is_empty() {
            return Err(DatabaseError::new(
                StateError,
                &format!("Expected to find a single unique account state, found {}", docs.len()),
            ));
        }
        Ok(models::AccountState::from(docs.pop().unwrap()))
    }

    fn get_deposits_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
        let query = doc!{ "slot" => slot };
        self.get_items_from_query(query, "deposits")
    }

    fn get_withdraws_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
        let query = doc!{ "slot" => slot };
        self.get_items_from_query(query, "withdraws")
    }

    fn get_orders_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::Order>, DatabaseError> {
        let query = doc!{ "auctionId" => slot };
        self.get_items_from_query(query, "orders")
    }
    fn get_standing_orders_of_slot(
        &self,
        slot: u32,
    ) -> Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError> {
        let pipeline = vec![
            doc!{"$match" => (doc!{"validFromAuctionId" => (doc!{ "$lte" => slot})})},
            doc!{"$sort" => (doc!{"validFromAuctionId" => -1, "_id" => -1})},
            doc!{"$group" => (doc!{"_id" => "$accountId", "orders" => (doc!{"$first" =>"$orders" }), "batchIndex" => (doc!{"$first" => "$batchIndex" })})}
        ];

        info!("Querying standing_orders: {:?}", pipeline);
        let mut standing_orders = models::StandingOrder::empty_array();
        let non_zero_standing_orders = self.client
            .db(models::DB_NAME)
            .collection("standing_orders")
            .aggregate(pipeline, None)
            .map_err(|e| DatabaseError::chain(ConnectionError, "Failed to get standing orders", e))?
            .map(|d| d.map(models::StandingOrder::from)
                .map_err(|e| DatabaseError::chain(StateError, "Failed to parse standing order", e))
            )
            .collect::<Result<Vec<_>, _>>()?;
        non_zero_standing_orders.into_iter().for_each(|k| {
            let acc_id = k.account_id as usize;
            standing_orders[acc_id] = k;
        });
        Ok(standing_orders)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use mock_it::Mock;

    #[derive(Clone)]
    pub struct DbInterfaceMock {
        pub get_current_balances: Mock<H256, Result<models::AccountState, DatabaseError>>,
        pub get_deposits_of_slot: Mock<u32, Result<Vec<models::PendingFlux>, DatabaseError>>,
        pub get_withdraws_of_slot: Mock<u32, Result<Vec<models::PendingFlux>, DatabaseError>>,
        pub get_orders_of_slot: Mock<u32, Result<Vec<models::Order>, DatabaseError>>,
        pub get_standing_orders_of_slot: Mock<u32, Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError>>,
    }

    impl DbInterfaceMock {
        pub fn new() -> DbInterfaceMock {
            DbInterfaceMock {
                get_current_balances: Mock::new(Err(DatabaseError::new(Unknown, "Unexpected call to get_current_balances"))),
                get_deposits_of_slot: Mock::new(Err(DatabaseError::new(Unknown, "Unexpected call to get_deposits_of_slot"))),
                get_withdraws_of_slot: Mock::new(Err(DatabaseError::new(Unknown, "Unexpected call to get_withdraws_of_slot"))),
                get_orders_of_slot: Mock::new(Err(DatabaseError::new(Unknown, "Unexpected call to get_withdraws_of_slot"))),
                get_standing_orders_of_slot: Mock::new(Err(DatabaseError::new(Unknown, "Unexpected call to get_standing_orders_of_slot"))),
            }
        }
    }

    impl DbInterface for DbInterfaceMock {
        fn get_current_balances(
            &self,
            current_state_root: &H256,
        ) -> Result<models::AccountState, DatabaseError> {
            self.get_current_balances.called(*current_state_root)  // https://github.com/intellij-rust/intellij-rust/issues/3164
        }
        fn get_deposits_of_slot(
            &self,
            slot: u32,
        ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
            self.get_deposits_of_slot.called(slot)
        }
        fn get_withdraws_of_slot(
            &self,
            slot: u32,
        ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
            self.get_withdraws_of_slot.called(slot)
        }
        fn get_orders_of_slot(
            &self,
            slot: u32,
        ) -> Result<Vec<models::Order>, DatabaseError> {
            self.get_orders_of_slot.called(slot)
        }
        fn get_standing_orders_of_slot(
            &self,
            slot: u32,
        ) -> Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError> {
            self.get_standing_orders_of_slot.called(slot)
        }
    }
}