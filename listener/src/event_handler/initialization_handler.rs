use super::*;

use dfusion_core::models::State;

use graph::data::store::{Entity};

#[derive(Debug, Clone)]
pub struct InitializationHandler {}

impl EventHandler for InitializationHandler {
    fn process_event(
        &self,
        logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>
    ) -> Result<Vec<EntityOperation>, Error> {
        info!(logger, "Processing Initialization Event {:?}", &log.data);
        let state = State::from(log);
        let entity: Entity = state.into();
        
        Ok(vec![
            EntityOperation::Set {
                key: util::entity_key(
                    "AccountState", 
                    &entity.get("id")
                        .and_then(|v| v.clone().as_string())
                        .unwrap()
                ),
                data: entity
            }
        ])
    }
}