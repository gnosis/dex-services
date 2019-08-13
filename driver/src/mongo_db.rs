#[cfg(test)]
extern crate mock_it;

use dfusion_core::models;
use dfusion_core::database::{DbInterface, DatabaseError, ErrorKind::*};

use mongodb::{bson, doc};
use mongodb::ordered::OrderedDocument;
use mongodb::db::ThreadedDatabase;
use mongodb::{Client, ThreadedClient};

use web3::types::{H256, U256};

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

    fn get_balances_for_query(
        &self,
        query: mongodb::Document,
    ) -> Result<models::AccountState, DatabaseError> {
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
}

impl DbInterface for MongoDB {
    fn get_balances_for_state_root(
        &self,
        state_root: &H256,
    ) -> Result<models::AccountState, DatabaseError> {
        let query = doc!{ "stateHash" => format!("{:x}", state_root) };
        info!("Querying stateHash: {}", query);
        self.get_balances_for_query(query)
    }

    fn get_balances_for_state_index(
        &self,
        state_index: &U256,
    ) -> Result<models::AccountState, DatabaseError> {
        let query = doc!{ "stateIndex" => state_index.low_u64() };
        info!("Querying stateIndex: {}", query);
        self.get_balances_for_query(query)
    }

    fn get_deposits_of_slot(
        &self,
        slot: &U256,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
        let query = doc!{ "slot" => slot.low_u64() };
        self.get_items_from_query(query, "deposits")
    }

    fn get_withdraws_of_slot(
        &self,
        slot: &U256,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
        let query = doc!{ "slot" => slot.low_u64() };
        self.get_items_from_query(query, "withdraws")
    }

    fn get_orders_of_slot(
        &self,
        slot: &U256,
    ) -> Result<Vec<models::Order>, DatabaseError> {
        let query = doc!{ "auctionId" => slot.low_u64() };
        self.get_items_from_query(query, "orders")
    }
    fn get_standing_orders_of_slot(
        &self,
        slot: &U256,
    ) -> Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError> {
        let pipeline = vec![
            doc!{"$match" => (doc!{"validFromAuctionId" => (doc!{ "$lte" => slot.low_u64()})})},
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