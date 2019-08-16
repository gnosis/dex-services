use super::*;
use crate::models::util::ToValue;
use crate::SUBGRAPH_ID;


use graph::components::store::{EntityFilter, EntityQuery, EntityRange, EntityOrder};
use graph::data::store::{Value, ValueType};

use graph_node_reader::StoreReader;
use crate::models::{NUM_RESERVED_ACCOUNTS, StandingOrder};

pub struct GraphReader {
    reader: Box<StoreReader>
}

impl GraphReader {
    pub fn new(reader: Box<StoreReader>) -> Self {
        GraphReader {
            reader
        }
    }

    fn get_flux_of_slot(
        &self,
        slot: &U256,
        flux: &str,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
        let deposit_query = entity_query(
            flux, EntityFilter::Equal("slot".to_string(), slot.to_value()),
        );
        Ok(self.reader
            .find(deposit_query)
            .map_err(|e| DatabaseError::chain(ErrorKind::ConnectionError, "Could not execute query", e))?
            .into_iter()
            .map(models::PendingFlux::from)
            .collect::<Vec<models::PendingFlux>>())
    }

    fn get_balances_for_query(
        &self,
        query: EntityQuery,
    ) -> Result<models::AccountState, DatabaseError> {
        Ok(models::AccountState::from(self.reader
            .find_one(query.clone())
            .map_err(|e| DatabaseError::chain(ErrorKind::ConnectionError, "Could not execute query", e))?
            .ok_or_else(|| DatabaseError::new(
                ErrorKind::StateError,
                &format!("No state record found for query {:?}", &query))
            )?
        ))
    }
}

impl DbInterface for GraphReader {
    fn get_balances_for_state_root(
        &self,
        state_root: &H256,
    ) -> Result<models::AccountState, DatabaseError> {
        let account_query = entity_query(
            "AccountState", EntityFilter::Equal("id".to_string(), state_root.to_value()),
        );
        self.get_balances_for_query(account_query)
    }

    fn get_balances_for_state_index(
        &self,
        state_index: &U256,
    ) -> Result<models::AccountState, DatabaseError> {
        let account_query = entity_query(
            "AccountState", EntityFilter::Equal("stateIndex".to_string(), state_index.to_value()),
        );
        self.get_balances_for_query(account_query)
    }

    fn get_deposits_of_slot(
        &self,
        slot: &U256,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
        self.get_flux_of_slot(slot, "Deposit")
    }

    fn get_withdraws_of_slot(
        &self,
        slot: &U256,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
        self.get_flux_of_slot(slot, "Withdraw")
    }

    fn get_orders_of_slot(
        &self,
        auction_id: &U256,
    ) -> Result<Vec<models::Order>, DatabaseError> {
        let order_query = util::entity_query(
            "SellOrder", EntityFilter::Equal("auctionId".to_string(), auction_id.to_value()),
        );
        Ok(self.reader
            .find(order_query)
            .map_err(|e| DatabaseError::chain(ErrorKind::ConnectionError, "Could not execute query", e))?
            .into_iter()
            .map(models::Order::from)
            .collect::<Vec<models::Order>>()
        )
    }

    fn get_standing_orders_of_slot(
        &self,
        auction_id: &U256,
    ) -> Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError> {
        let search_filter = EntityFilter::LessOrEqual("validFromAuctionId".to_string(), auction_id.to_value());
        let mut order_ids: Vec<ValueType::ID> = vec![];

//        (0..NUM_RESERVED_ACCOUNTS).map(|reserved_account_id| {
//            let standing_order_query = EntityQuery {
//                subgraph_id: SUBGRAPH_ID.clone(),
//                entity_types: vec!["StandingSellOrderBatch".to_string()],
//                filter: Some(
//                    EntityFilter::And(
//                        vec![
//                            search_filter,
//                            EntityFilter::Equal("accountId".to_string(), (reserved_account_id as u16).to_value())
//                        ]
//                    )
//                ),
//                order_by: Some(("batchIndex".to_string(), ValueType::BigInt)),
//                order_direction: Some(EntityOrder::Descending),
//                range: EntityRange {
//                    first: None,
//                    skip: 0,
//                },
//            };
//            let standing_order_entity = self.reader
//                .find_one(standing_order_query)
//                .map_err(|e| DatabaseError::chain(ErrorKind::ConnectionError, "Could not execute query", e))?
//
//            match standing_order_entity {
//                Some(standing_order) => {
//                    let order_ids = standing_order.get("orders").as_list()?;
//                    let relevant_order_query = entity_query("SellOrder", EntityFilter::In("id".to_string(), order_ids));
//                    let order_entities = self.reader
//                        .find(relevant_order_query)
//                        .map_err(|e| DatabaseError::chain(ErrorKind::ConnectionError, "Could not execute query", e))?
//
//                    StandingOrder::from((standing_order_entity, order_entities))
//                },
//                None => StandingOrder::Default()
//            }
//        })
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