use graph::components::store::{EntityFilter, EntityOrder, EntityQuery, EntityRange};
use graph::data::store::ValueType;
use web3::types::H160;

use super::*;
use crate::models::util::ToValue;
use crate::SUBGRAPH_ID;
use crate::models::StandingOrder;
use graph_node_reader::StoreReader;

pub struct GraphReader {
    reader: Box<dyn StoreReader>,
}

impl GraphReader {
    pub fn new(reader: Box<dyn StoreReader>) -> Self {
        GraphReader { reader }
    }

    fn get_flux_of_slot(
        &self,
        slot: &U256,
        flux: &str,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
        let deposit_query = entity_query(
            flux,
            EntityFilter::Equal("slot".to_string(), slot.to_value()),
        );
        Ok(self
            .reader
            .find(deposit_query)
            .map_err(|e| {
                DatabaseError::chain(ErrorKind::ConnectionError, "Could not execute query", e)
            })?
            .into_iter()
            .map(models::PendingFlux::from)
            .collect::<Vec<models::PendingFlux>>())
    }

    fn get_balances_for_query(
        &self,
        query: EntityQuery,
    ) -> Result<models::AccountState, DatabaseError> {
        Ok(models::AccountState::from(
            self.reader
                .find_one(query.clone())
                .map_err(|e| {
                    DatabaseError::chain(ErrorKind::ConnectionError, "Could not execute query", e)
                })?
                .ok_or_else(|| {
                    DatabaseError::new(
                        ErrorKind::StateError,
                        &format!("No state record found for query {:?}", &query),
                    )
                })?,
        ))
    }
}

impl DbInterface for GraphReader {
    fn get_balances_for_state_root(
        &self,
        state_root: &H256,
    ) -> Result<models::AccountState, DatabaseError> {
        let account_query = entity_query(
            "AccountState",
            EntityFilter::Equal("id".to_string(), state_root.to_value()),
        );
        self.get_balances_for_query(account_query)
    }

    fn get_balances_for_state_index(
        &self,
        state_index: &U256,
    ) -> Result<models::AccountState, DatabaseError> {
        let account_query = entity_query(
            "AccountState",
            EntityFilter::Equal("stateIndex".to_string(), state_index.to_value()),
        );
        self.get_balances_for_query(account_query)
    }

    fn get_deposits_of_slot(&self, slot: &U256) -> Result<Vec<models::PendingFlux>, DatabaseError> {
        self.get_flux_of_slot(slot, "Deposit")
    }

    fn get_withdraws_of_slot(
        &self,
        slot: &U256,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
        self.get_flux_of_slot(slot, "Withdraw")
    }

    fn get_orders_of_slot(&self, auction_id: &U256) -> Result<Vec<models::Order>, DatabaseError> {
        let order_query = entity_query(
            "SellOrder",
            EntityFilter::Equal("auctionId".to_string(), auction_id.to_value()),
        );
        Ok(self
            .reader
            .find(order_query)
            .map_err(|e| {
                DatabaseError::chain(ErrorKind::ConnectionError, "Could not execute query", e)
            })?
            .into_iter()
            .map(models::Order::from)
            .collect::<Vec<models::Order>>())
    }

    fn get_standing_orders_of_slot(
        &self,
        auction_id: &U256,
    ) -> Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError> {
        let mut result = StandingOrder::empty_array();
        for (reserved_account_id, item) in result.iter_mut().enumerate() {
            let standing_order_query = EntityQuery {
                subgraph_id: SUBGRAPH_ID.clone(),
                entity_types: vec!["StandingSellOrderBatch".to_string()],
                filter: Some(EntityFilter::And(vec![
                    EntityFilter::LessOrEqual(
                        "validFromAuctionId".to_string(),
                        auction_id.to_value(),
                    ),
                    EntityFilter::Equal(
                        "accountId".to_string(),
                        H160::from_low_u64_be(reserved_account_id as _).to_value(),
                    ),
                ])),
                order_by: Some(("batchIndex".to_string(), ValueType::BigInt)),
                order_direction: Some(EntityOrder::Descending),
                range: EntityRange {
                    first: None,
                    skip: 0,
                },
            };
            let standing_order_option =
                self.reader.find_one(standing_order_query).map_err(|e| {
                    DatabaseError::chain(ErrorKind::ConnectionError, "Could not execute query", e)
                })?;

            if let Some(standing_order) = standing_order_option {
                let order_ids = standing_order
                    .get("orders")
                    .and_then(|orders| orders.clone().as_list())
                    .ok_or_else(|| {
                        DatabaseError::new(
                            ErrorKind::StateError,
                            "No list orders found on standing order entity",
                        )
                    })?;
                let relevant_order_query =
                    entity_query("SellOrder", EntityFilter::In("id".to_string(), order_ids));
                let order_entities = self.reader.find(relevant_order_query).map_err(|e| {
                    DatabaseError::chain(ErrorKind::ConnectionError, "Could not execute query", e)
                })?;
                *item = StandingOrder::from((standing_order, order_entities));
            }
        }
        Ok(result)
    }
}

pub fn entity_query(entity_type: &str, filter: EntityFilter) -> EntityQuery {
    EntityQuery {
        subgraph_id: SUBGRAPH_ID.clone(),
        entity_types: vec![entity_type.to_string()],
        filter: Some(filter),
        order_by: None,
        order_direction: None,
        range: EntityRange {
            first: None,
            skip: 0,
        },
    }
}
