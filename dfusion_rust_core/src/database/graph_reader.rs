use super::*;
use crate::models::util::ToValue;
use crate::SUBGRAPH_ID;

use graph::components::store::{EntityFilter, EntityQuery, EntityRange};

use graph_node_reader::StoreReader;

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
                    flux, EntityFilter::Equal("slot".to_string(), slot.to_value())
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
        query: EntityQuery
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
            "AccountState", EntityFilter::Equal("stateRoot".to_string(), state_root.to_value())
        );
        self.get_balances_for_query(account_query)
    }

    fn get_balances_for_state_index(
        &self,
        state_index: &U256,
    ) -> Result<models::AccountState, DatabaseError> {
        let account_query = entity_query(
            "AccountState", EntityFilter::Equal("stateIndex".to_string(), state_index.to_value())
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
        _: &U256,
    ) -> Result<Vec<models::Order>, DatabaseError> {
        unimplemented!()
    }

    fn get_standing_orders_of_slot(
        &self,
        _: &U256,
    ) -> Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError> {
        unimplemented!()
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
            skip: 0
        }
    }
}