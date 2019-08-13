use diesel::pg::PgConnection;
use diesel::r2d2::{self, ConnectionManager, Pool, PooledConnection};
use std::time::{Duration, Instant};

use graph::prelude::*;
use graph::serde_json;
use graph::util::security::SafeDisplay;

use crate::entities as e;

/// A Store based on Diesel and Postgres.
pub struct Store {
    logger: Logger,
    // listen to StoreEvents generated when applying entity operations
    conn: Pool<ConnectionManager<PgConnection>>,
}

impl Store {
    pub fn new(
        postgres_url: String,
        logger: &Logger,
    ) -> Self {
        // Create a store-specific logger
        let logger = logger.new(o!("component" => "Store"));

        #[derive(Debug)]
        struct ErrorHandler(Logger);
        impl r2d2::HandleError<r2d2::Error> for ErrorHandler {
            fn handle_error(&self, error: r2d2::Error) {
                error!(self.0, "Postgres connection error"; "error" => error.to_string())
            }
        }
        let error_handler = Box::new(ErrorHandler(logger.clone()));

        // Connect to Postgres
        let conn_manager = ConnectionManager::new(postgres_url.as_str());
        let pool = Pool::builder()
            .error_handler(error_handler)
            // Set the time we wait for a connection to 6h. The default is 30s
            // which can be too little if database connections are highly
            // contended; if we don't get a connection within the timeout,
            // ultimately subgraphs get marked as failed. This effectively
            // turns off this timeout and makes it possible that work needing
            // a database connection blocks for a very long time
            .connection_timeout(Duration::from_secs(6 * 60 * 60))
            .build(conn_manager)
            .unwrap();
        info!(
            logger,
            "Connected to Postgres";
            "url" => SafeDisplay(postgres_url.as_str())
        );

        // Create the store
        let store = Store {
            logger: logger.clone(),
            conn: pool,
        };
        // Return the store
        store
    }

    /// Gets an entity from Postgres.
    fn get_entity(
        &self,
        conn: &e::Connection,
        op_subgraph: &SubgraphDeploymentId,
        op_entity: &String,
        op_id: &String,
    ) -> Result<Option<Entity>, QueryExecutionError> {
        match conn.find(op_subgraph, op_entity, op_id).map_err(|e| {
            QueryExecutionError::ResolveEntityError(
                op_subgraph.clone(),
                op_entity.clone(),
                op_id.clone(),
                format!("{}", e),
            )
        })? {
            Some(json) => {
                let mut value = serde_json::from_value::<Entity>(json).map_err(|e| {
                    QueryExecutionError::ResolveEntityError(
                        op_subgraph.clone(),
                        op_entity.clone(),
                        op_id.clone(),
                        format!("Invalid entity: {}", e),
                    )
                })?;
                value.set("__typename", op_entity);
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    fn execute_query(
        &self,
        conn: &e::Connection,
        query: EntityQuery,
    ) -> Result<Vec<Entity>, QueryExecutionError> {
        // Add order by filters to query
        let order = match query.order_by {
            Some((attribute, value_type)) => {
                let direction = query
                    .order_direction
                    .map(|direction| match direction {
                        EntityOrder::Ascending => "ASC",
                        EntityOrder::Descending => "DESC",
                    })
                    .unwrap_or("ASC");
                let cast_type = match value_type {
                    ValueType::BigInt | ValueType::BigDecimal => "::numeric",
                    ValueType::Boolean => "::boolean",
                    ValueType::Bytes => "",
                    ValueType::ID => "",
                    ValueType::Int => "::bigint",
                    ValueType::String => "",
                    ValueType::List => {
                        return Err(QueryExecutionError::OrderByNotSupportedForType(
                            "List".to_string(),
                        ));
                    }
                };
                Some((attribute, cast_type, direction))
            }
            None => None,
        };

        // Process results; deserialize JSON data
        conn.query(
            &query.subgraph_id,
            query.entity_types,
            query.filter,
            order,
            query.range.first,
            query.range.skip,
        )
        .map(|values| {
            values
                .into_iter()
                .map(|(value, entity_type)| {
                    let parse_error_msg = format!("Error parsing entity JSON: {:?}", value);
                    let mut value =
                        serde_json::from_value::<Entity>(value).expect(&parse_error_msg);
                    value.set("__typename", entity_type);
                    value
                })
                .collect()
        })
    }

    fn get_conn(&self) -> Result<PooledConnection<ConnectionManager<PgConnection>>, Error> {
        let start_time = Instant::now();
        let conn = self.conn.get();
        let wait = start_time.elapsed();
        if wait > Duration::from_millis(10) {
            warn!(self.logger, "Possible contention in DB connection pool";
                               "wait_ms" => wait.as_millis())
        }
        conn.map_err(Error::from)
    }
}

/// Common trait for store implementations.
pub trait StoreReader: Send + Sync + 'static {
    /// Looks up an entity using the given store key.
    fn get(&self, key: EntityKey) -> Result<Option<Entity>, QueryExecutionError>;

    /// Queries the store for entities that match the store query.
    fn find(&self, query: EntityQuery) -> Result<Vec<Entity>, QueryExecutionError>;

    /// Queries the store for a single entity matching the store query.
    fn find_one(&self, query: EntityQuery) -> Result<Option<Entity>, QueryExecutionError>;
}

impl StoreReader for Store {

    fn get(&self, key: EntityKey) -> Result<Option<Entity>, QueryExecutionError> {
        let conn = self
            .get_conn()
            .map_err(|e| QueryExecutionError::StoreError(e.into()))?;
        let conn = e::Connection::new(&conn);
        self.get_entity(&conn, &key.subgraph_id, &key.entity_type, &key.entity_id)
    }

    fn find(&self, query: EntityQuery) -> Result<Vec<Entity>, QueryExecutionError> {
        let conn = self
            .get_conn()
            .map_err(|e| QueryExecutionError::StoreError(e.into()))?;
        let conn = e::Connection::new(&conn);
        self.execute_query(&conn, query)
    }

    fn find_one(&self, mut query: EntityQuery) -> Result<Option<Entity>, QueryExecutionError> {
        query.range = EntityRange::first(1);

        let conn = self
            .get_conn()
            .map_err(|e| QueryExecutionError::StoreError(e.into()))?;
        let conn = e::Connection::new(&conn);

        let mut results = self.execute_query(&conn, query)?;
        match results.len() {
            0 | 1 => Ok(results.pop()),
            n => panic!("find_one query found {} results", n),
        }
    }
}