mod event_handler;
mod link_resolver;
mod metrics_registry;
mod runtime_host;

use lazy_static::lazy_static;
use std::any::type_name;
use std::collections::HashMap;
use std::env;
use std::str::FromStr;
use std::time::Duration;
use tokio_timer::timer::Timer;

use graph::components::forward;
use graph::log::logger;
use graph::prelude::{SubgraphRegistrar as _, *};
use graph::util::security::SafeDisplay;
use graph_core::{SubgraphAssignmentProvider, SubgraphInstanceManager, SubgraphRegistrar};
use graph_datasource_ethereum::{BlockStreamBuilder, Transport};
use graph_server_http::GraphQLServer as GraphQLQueryServer;
use graph_server_websocket::SubscriptionServer as GraphQLSubscriptionServer;
use graph_store_postgres::connection_pool::create_connection_pool;
use graph_store_postgres::{Store as DieselStore, StoreConfig};

use crate::link_resolver::LocalLinkResolver;
use crate::metrics_registry::SimpleMetricsRegistry;
use crate::runtime_host::RustRuntimeHostBuilder;

use dfusion_core::database::GraphReader;
use dfusion_core::SUBGRAPH_NAME;
use graph_node_reader::Store as GraphNodeReader;

lazy_static! {
    static ref REORG_THRESHOLD: u64 = 50;
    static ref ANCESTOR_COUNT: u64 = 50;
    static ref TOKIO_THREAD_COUNT: usize = 100;
    static ref SUBGRAPH_ID: SubgraphDeploymentId =
        SubgraphDeploymentId::new(SUBGRAPH_NAME).unwrap();
    static ref NODE_ID: NodeId = NodeId::new("default").unwrap();
}

fn main() {
    use std::sync::Mutex;
    use tokio::runtime;

    // Create components for tokio context: multi-threaded runtime, executor
    // context on the runtime, and Timer handle.
    //
    // Configure the runtime to shutdown after a panic.
    let runtime: Arc<Mutex<Option<runtime::Runtime>>> = Arc::new(Mutex::new(None));
    let handler_runtime = runtime.clone();
    *runtime.lock().unwrap() = Some(
        runtime::Builder::new()
            .core_threads(*TOKIO_THREAD_COUNT)
            .panic_handler(move |_| {
                let runtime = handler_runtime.clone();
                std::thread::spawn(move || {
                    if let Some(runtime) = runtime.lock().unwrap().take() {
                        // Try to cleanly shutdown the runtime, but
                        // unconditionally exit after a while.
                        std::thread::spawn(|| {
                            std::thread::sleep(Duration::from_millis(3000));
                            std::process::exit(1);
                        });
                        runtime
                            .shutdown_now()
                            .wait()
                            .expect("Failed to shutdown Tokio Runtime");
                        println!("Runtime cleaned up and shutdown successfully");
                    }
                });
            })
            .build()
            .unwrap(),
    );

    let mut executor = runtime.lock().unwrap().as_ref().unwrap().executor();
    let mut enter = tokio_executor::enter()
        .expect("Failed to enter runtime executor, multiple executors at once");
    let timer = Timer::default();
    let timer_handle = timer.handle();

    // Setup runtime context with defaults and run the main application
    tokio_executor::with_default(&mut executor, &mut enter, |enter| {
        tokio_timer::with_default(&timer_handle, enter, |enter| {
            enter
                .block_on(future::lazy(|| async_main()))
                .expect("Failed to run main function");
        })
    });
}

fn async_main() -> impl Future<Item = (), Error = ()> + Send + 'static {
    env_logger::init();

    // just read from env instead of using command line arguments
    let postgres_url: String = env_arg("POSTGRES_URL");
    let ethereum_rpc: String = env_arg("ETHEREUM_NODE_URL");
    let network_name: String = env_arg("NETWORK_NAME");

    let http_port: u16 = env_arg("GRAPHQL_PORT");
    let ws_port: u16 = env_arg("WS_PORT");

    // Set up logger
    let logger = logger(false);

    // Some hard-coded options
    let block_polling_interval = Duration::from_millis(500);
    let store_conn_pool_size = 10;

    info!(logger, "Starting up");

    let logger_factory = LoggerFactory::new(logger.clone(), None);

    // Create a local link resolver (IPFS is not used)
    let link_resolver = Arc::new(LocalLinkResolver);

    // Set up simple metrics registry
    let metrics_registry = Arc::new(SimpleMetricsRegistry);

    // Ethereum client
    let eth_adapter: Arc<dyn EthereumAdapter> = {
        let (event_loop, transport) = Transport::new_rpc(&ethereum_rpc);
        event_loop.into_remote();

        Arc::new(graph_datasource_ethereum::EthereumAdapter::new(
            transport,
            Arc::new(ProviderEthRpcMetrics::new(metrics_registry.clone())),
        ))
    };
    let eth_net_identifier = match eth_adapter.net_identifiers(&logger).wait() {
        Ok(network_identifier) => {
            info!(
                logger,
                "Connected to Ethereum";
                "network" => &network_name,
                "network_version" => &network_identifier.net_version,
            );
            network_identifier
        }
        Err(e) => {
            error!(logger, "Was a valid Ethereum node provided?");
            panic!("Failed to connect to Ethereum node: {}", e);
        }
    };

    // Set up Store
    info!(
        logger,
        "Connecting to Postgres";
        "url" => SafeDisplay(postgres_url.as_str()),
        "conn_pool_size" => store_conn_pool_size,
    );
    let postgres_conn_pool =
        create_connection_pool(postgres_url.clone(), store_conn_pool_size, &logger);
    let generic_store = Arc::new(DieselStore::new(
        StoreConfig {
            postgres_url: postgres_url.clone(),
            network_name: network_name.clone(),
        },
        &logger,
        eth_net_identifier,
        postgres_conn_pool.clone(),
    ));

    let eth_adapters: HashMap<String, Arc<dyn EthereumAdapter>> = {
        let mut eth_adapters = HashMap::new();
        eth_adapters.insert(network_name.clone(), eth_adapter.clone());
        eth_adapters
    };
    let stores: HashMap<String, Arc<DieselStore>> = {
        let mut stores = HashMap::new();
        stores.insert(network_name.clone(), generic_store.clone());
        stores
    };

    let graphql_runner = Arc::new(graph_core::GraphQlRunner::new(
        &logger,
        generic_store.clone(),
    ));
    let mut graphql_server = GraphQLQueryServer::new(
        &logger_factory,
        graphql_runner.clone(),
        generic_store.clone(),
        NODE_ID.clone(),
    );
    let mut subscription_server =
        GraphQLSubscriptionServer::new(&logger, graphql_runner.clone(), generic_store.clone());

    info!(logger, "Starting block ingestor");

    // Create Ethereum block ingestor and spawn a thread to it
    let block_ingestor = graph_datasource_ethereum::BlockIngestor::new(
        generic_store.clone(),
        eth_adapter.clone(),
        *ANCESTOR_COUNT,
        network_name.to_string(),
        &logger_factory,
        block_polling_interval,
    )
    .expect("failed to create Ethereum block ingestor");

    // Run the Ethereum block ingestor in the background
    tokio::spawn(block_ingestor.into_polling_stream());

    let block_stream_builder = BlockStreamBuilder::new(
        generic_store.clone(),
        stores.clone(),
        eth_adapters.clone(),
        NODE_ID.clone(),
        *REORG_THRESHOLD,
        metrics_registry.clone(),
    );

    let runtime_host_builder = {
        let store_reader = Box::new(GraphNodeReader::new(postgres_url.clone(), &logger));
        let database = Arc::new(GraphReader::new(store_reader));
        RustRuntimeHostBuilder::new(database)
    };

    let subgraph_instance_manager = SubgraphInstanceManager::new(
        &logger_factory,
        stores.clone(),
        eth_adapters.clone(),
        runtime_host_builder,
        block_stream_builder,
        metrics_registry.clone(),
    );

    // Create subgraph provider
    let mut subgraph_provider = SubgraphAssignmentProvider::new(
        &logger_factory,
        link_resolver.clone(),
        generic_store.clone(),
        graphql_runner.clone(),
    );

    // Forward subgraph events from the subgraph provider to the subgraph instance manager
    tokio::spawn(forward(&mut subgraph_provider, &subgraph_instance_manager).unwrap());

    // Create named subgraph provider for resolving subgraph name->ID mappings
    let subgraph_registrar = Arc::new(SubgraphRegistrar::new(
        &logger_factory,
        link_resolver,
        Arc::new(subgraph_provider),
        generic_store.clone(),
        stores,
        eth_adapters.clone(),
        NODE_ID.clone(),
        SubgraphVersionSwitchingMode::Instant,
    ));
    tokio::spawn(
        subgraph_registrar
            .start()
            .then(|start_result| Ok(start_result.expect("failed to initialize subgraph provider"))),
    );

    // Add the dfusion subgraph.
    let subgraph_name = SubgraphName::new(SUBGRAPH_NAME).unwrap();
    tokio::spawn(
        subgraph_registrar
            .create_subgraph(subgraph_name.clone())
            .then(|result| Ok(result.expect("Failed to create subgraph")))
            .and_then(move |_| {
                subgraph_registrar.create_subgraph_version(
                    subgraph_name,
                    SUBGRAPH_ID.clone(),
                    NODE_ID.clone(),
                )
            })
            .then(|result| Ok(result.expect("Failed to deploy subgraph"))),
    );

    // Serve GraphQL queries over HTTP
    tokio::spawn(
        graphql_server
            .serve(http_port, ws_port)
            .expect("Failed to start GraphQL query server"),
    );

    // Serve GraphQL subscriptions over WebSockets
    tokio::spawn(
        subscription_server
            .serve(ws_port)
            .expect("Failed to start GraphQL subscription server"),
    );

    // // Periodically check for contention in the tokio threadpool. First spawn a
    // // task that simply responds to "ping" requests. Then spawn a separate
    // // thread to periodically ping it and check responsiveness.
    // let (ping_send, ping_receive) = mpsc::channel::<crossbeam_channel::Sender<()>>(1);
    // tokio::spawn(
    //     ping_receive
    //         .for_each(move |pong_send| pong_send.clone().send(()).map(|_| ()).map_err(|_| ())),
    // );
    // let contention_logger = logger.clone();
    // std::thread::spawn(move || loop {
    //     std::thread::sleep(Duration::from_secs(1));
    //     let (pong_send, pong_receive) = crossbeam_channel::bounded(1);
    //     if ping_send.clone().send(pong_send).wait().is_err() {
    //         debug!(contention_logger, "Shutting down contention checker thread");
    //         break;
    //     }
    //     let mut timeout = Duration::from_millis(10);
    //     while pong_receive.recv_timeout(timeout)
    //         == Err(crossbeam_channel::RecvTimeoutError::Timeout)
    //     {
    //         debug!(
    //             contention_logger,
    //             "Possible contention in tokio threadpool";
    //             "timeout_ms" => timeout.as_millis(),
    //             "code" => LogCode::TokioContention,
    //         );
    //         if timeout < Duration::from_secs(10) {
    //             timeout *= 10;
    //         }
    //     }
    // });

    future::empty()
}

fn env_arg<V>(name: &str) -> V
where
    V: FromStr,
    V::Err: Debug,
{
    let value = env::var(name).expect(&format!("{} environment variable is required", name));
    value.parse::<V>().expect(&format!(
        "failed to parse environment variable {}='{}' as {}",
        name,
        value,
        type_name::<V>()
    ))
}
