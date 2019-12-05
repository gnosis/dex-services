mod event_handler;
mod link_resolver;
mod runtime_host;

use lazy_static::lazy_static;
use std::collections::HashMap;
use std::env::{self, VarError};
use std::str::FromStr;
use std::time::Duration;
use tokio_timer::timer::Timer;
use std::any::type_name;

use graph::components::forward;
use graph::log::logger;
use graph::prelude::{
    SubgraphRegistrar as _, *,
};
use graph::util::security::SafeDisplay;
use graph_core::{
    SubgraphAssignmentProvider,
    SubgraphInstanceManager, SubgraphRegistrar,
};
use graph_datasource_ethereum::{BlockStreamBuilder, Transport};
use graph_mock::MockMetricsRegistry;
use graph_server_http::GraphQLServer as GraphQLQueryServer;
use graph_server_websocket::SubscriptionServer as GraphQLSubscriptionServer;
use graph_store_postgres::connection_pool::create_connection_pool;
use graph_store_postgres::{Store as DieselStore, StoreConfig};

use crate::link_resolver::LocalLinkResolver;
use crate::runtime_host::RustRuntimeHostBuilder;

use dfusion_core::database::GraphReader;
use dfusion_core::SUBGRAPH_NAME;
use graph_node_reader::Store as GraphNodeReader;

lazy_static! {
    static ref REORG_THRESHOLD: u64 = 50;
    static ref ANCESTOR_COUNT: u64 = 50;
    static ref TOKIO_THREAD_COUNT: usize = 100;

    static ref SUBGRAPH_ID: SubgraphDeploymentId = SubgraphDeploymentId::new(SUBGRAPH_NAME).unwrap();
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
    let postgres_url: String = env_arg("POSTGRES_URL", None);
    let ethereum_rpc: String = env_arg("ETHEREUM_NODE_URL", None);
    let network_name: String = env_arg("NETWORK_NAME", None);

    let http_port: u16 = env_arg("GRAPHQL_PORT", None);
    let ws_port: u16 = env_arg("WS_PORT", None);

    // Set up logger
    let logger = logger(false);

    // Some hard-coded options
    let block_polling_interval = Duration::from_millis(500);
    let json_rpc_port = 8020;
    let index_node_port = 8030;
    let metrics_port = 8040;
    let disable_block_ingestor = false;
    let store_conn_pool_size = 10;

    info!(logger, "Starting up");

    // // Parse the IPFS URL from the `--ipfs` command line argument
    // let ipfs_address = matches
    //     .value_of("ipfs")
    //     .map(|uri| {
    //         if uri.starts_with("http://") || uri.starts_with("https://") {
    //             String::from(uri)
    //         } else {
    //             format!("http://{}", uri)
    //         }
    //     })
    //     .unwrap()
    //     .to_owned();

    let logger_factory = LoggerFactory::new(logger.clone(), None);

    // info!(
    //     logger,
    //     "Trying IPFS node at: {}",
    //     SafeDisplay(&ipfs_address)
    // );

    // // Try to create an IPFS client for this URL
    // let ipfs_client = match IpfsClient::new_from_uri(ipfs_address.as_ref()) {
    //     Ok(ipfs_client) => ipfs_client,
    //     Err(e) => {
    //         error!(
    //             logger,
    //             "Failed to create IPFS client for `{}`: {}",
    //             SafeDisplay(&ipfs_address),
    //             e
    //         );
    //         panic!("Could not connect to IPFS");
    //     }
    // };

    // // Test the IPFS client by getting the version from the IPFS daemon
    // let ipfs_test = ipfs_client.version();
    // let ipfs_ok_logger = logger.clone();
    // let ipfs_err_logger = logger.clone();
    // let ipfs_address_for_ok = ipfs_address.clone();
    // let ipfs_address_for_err = ipfs_address.clone();
    // tokio::spawn(
    //     ipfs_test
    //         .map_err(move |e| {
    //             error!(
    //                 ipfs_err_logger,
    //                 "Is there an IPFS node running at \"{}\"?",
    //                 SafeDisplay(ipfs_address_for_err),
    //             );
    //             panic!("Failed to connect to IPFS: {}", e);
    //         })
    //         .map(move |_| {
    //             info!(
    //                 ipfs_ok_logger,
    //                 "Successfully connected to IPFS node at: {}",
    //                 SafeDisplay(ipfs_address_for_ok)
    //             );
    //         }),
    // );

    // // Convert the client into a link resolver
    // let link_resolver = Arc::new(LinkResolver::from(ipfs_client));
    let link_resolver = Arc::new(LocalLinkResolver {});

    // // Set up Prometheus registry
    // let prometheus_registry = Arc::new(Registry::new());
    // let metrics_registry = Arc::new(MetricsRegistry::new(
    //     logger.clone(),
    //     prometheus_registry.clone(),
    // ));
    // let mut metrics_server =
    //     PrometheusMetricsServer::new(&logger_factory, prometheus_registry.clone());
    let metrics_registry = Arc::new(MockMetricsRegistry::new());

    // Ethereum client
    let eth_adapter: Arc<dyn EthereumAdapter> = {
        let (event_loop, transport) = Transport::new_rpc(&ethereum_rpc);
        event_loop.into_remote();

        Arc::new(graph_datasource_ethereum::EthereumAdapter::new(
            transport,
            Arc::new(ProviderEthRpcMetrics::new(metrics_registry)),
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

    // let mut index_node_server = IndexNodeServer::new(
    //     &logger_factory,
    //     graphql_runner.clone(),
    //     generic_store.clone(),
    //     node_id.clone(),
    // );

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
    // let runtime_host_builder =
    //     WASMRuntimeHostBuilder::new(eth_adapters.clone(), link_resolver.clone(), stores.clone());

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

    // // Start admin JSON-RPC server.
    // let json_rpc_server = JsonRpcServer::serve(
    //     json_rpc_port,
    //     http_port,
    //     ws_port,
    //     subgraph_registrar.clone(),
    //     node_id.clone(),
    //     logger.clone(),
    // )
    // .expect("failed to start JSON-RPC admin server");

    // // Let the server run forever.
    // std::mem::forget(json_rpc_server);

    // Add the dfusion subgraph.
    let subgraph_name = SubgraphName::new(SUBGRAPH_NAME).unwrap();
    tokio::spawn(
        subgraph_registrar
            .create_subgraph(subgraph_name.clone())
            .then(|result| Ok(result.expect("Failed to create subgraph")))
            .and_then(move |_| {
                subgraph_registrar.create_subgraph_version(subgraph_name, SUBGRAPH_ID.clone(), NODE_ID.clone())
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

    // // Run the index node server
    // tokio::spawn(
    //     index_node_server
    //         .serve(index_node_port)
    //         .expect("Failed to start index node server"),
    // );

    // tokio::spawn(
    //     metrics_server
    //         .serve(metrics_port)
    //         .expect("Failed to start metrics server"),
    // );

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

fn env_arg<V>(name: &str, default: Option<V>) -> V
where
    V: FromStr,
    V::Err: Debug,
{
    let value = match env::var(name) {
        Ok(value) => value,
        Err(VarError::NotPresent) => {
            return default.expect(&format!("{} environment variable is required", name));
        }
        Err(VarError::NotUnicode(_)) => {
            panic!("{} environment variable contains invalid unicode", name)
        }
    };
    value.parse::<V>().expect(&format!(
        "failed to parse environment variable {}='{}' as {}",
        name,
        value,
        type_name::<V>()
    ))
}

/*
lazy_static! {
    static ref ANCESTOR_COUNT: u64 = 50;
    static ref REORG_THRESHOLD: u64 = 50;
    static ref BLOCK_POLLING_INTERVAL: Duration = Duration::from_millis(1000);
    static ref NODE_ID: NodeId = NodeId::new("default").unwrap();
    static ref SUBGRAPH_ID: SubgraphDeploymentId =
        SubgraphDeploymentId::new(SUBGRAPH_NAME).unwrap();
}

fn async_main() -> impl Future<Item = (), Error = ()> + Send + 'static {
    env_logger::init();

    let postgres_url = env::var("POSTGRES_URL").expect("Specify POSTGRES_URL variable");
    let ethereum_node_url = env::var("ETHEREUM_NODE_URL").expect("Specify ETHEREUM_RPC variable");
    let network_name = env::var("NETWORK_NAME").expect("Specify NETWORK_NAME variable");
    let store_conn_pool_size = env::var("STORE_CONNECTION_POOL_SIZE")
        .map_err(|e| e.to_string())
        .and_then(|size_str| size_str.parse::<u32>().map_err(|e| e.to_string()))
        .unwrap_or(10);

    let http_port = env::var("GRAPHQL_PORT")
        .expect("Specify GRAPHQL_PORT variable")
        .parse::<u16>()
        .expect("Couldn't parse GRAPHQL_PORT variable as u16");
    let ws_port = env::var("WS_PORT")
        .expect("Specify WS_PORT variable")
        .parse::<u16>()
        .expect("Couldn't parse WS_PORT variable as u16");

    let logger = logger(false);
    let logger_factory = LoggerFactory::new(logger.clone(), None);

    // Set up Ethereum transport
    let (transport_event_loop, transport) = Transport::new_rpc(&ethereum_node_url);

    // If we drop the event loop the transport will stop working.
    // For now it's fine to just leak it.
    std::mem::forget(transport_event_loop);

    let eth_adapter = Arc::new(graph_datasource_ethereum::EthereumAdapter::new(
        transport,
        unimplemented!(),
    ));
    let eth_net_identifier = match eth_adapter.net_identifiers(&logger).wait() {
        Ok(net) => {
            info!(
                logger, "Connected to Ethereum";
            );
            net
        }
        Err(e) => {
            error!(logger, "Was a valid Ethereum node provided?");
            panic!("Failed to connect to Ethereum node: {}", e);
        }
    };

    let postgres_conn_pool =
        create_connection_pool(postgres_url.clone(), store_conn_pool_size, &logger);
    let store = Arc::new(DieselStore::new(
        StoreConfig {
            postgres_url: postgres_url.clone(),
            network_name: network_name.clone(),
        },
        &logger,
        eth_net_identifier,
        postgres_conn_pool.clone(),
    ));

    // Create Ethereum block ingestor
    let block_ingestor = graph_datasource_ethereum::BlockIngestor::new(
        store.clone(),
        eth_adapter.clone(),
        *ANCESTOR_COUNT,
        network_name,
        &logger_factory,
        *BLOCK_POLLING_INTERVAL,
    )
    .expect("failed to create Ethereum block ingestor");

    // Run the Ethereum block ingestor in the background
    tokio::spawn(block_ingestor.into_polling_stream());

    // Prepare a block stream builder for subgraphs
    let block_stream_builder = BlockStreamBuilder::new(
        store.clone(),
        store.clone(),
        eth_adapter.clone(),
        NODE_ID.clone(),
        *REORG_THRESHOLD,
    );

    let store_reader = GraphNodeReader::new(postgres_url, &logger);
    let database = Arc::new(GraphReader::new(Box::new(store_reader)));
    let subgraph_instance_manager = SubgraphInstanceManager::new(
        &logger_factory,
        store.clone(),
        RustRuntimeHostBuilder::new(database),
        block_stream_builder,
    );

    let link_resolver = Arc::new(LocalLinkResolver {});
    let graphql_runner = Arc::new(graph_core::GraphQlRunner::new(&logger, store.clone()));
    let mut subgraph_provider = SubgraphAssignmentProvider::new(
        &logger_factory,
        link_resolver.clone(),
        store.clone(),
        graphql_runner.clone(),
    );

    // Forward subgraph events from the subgraph provider to the subgraph instance manager
    tokio::spawn(forward(&mut subgraph_provider, &subgraph_instance_manager).unwrap());
    let subgraph_provider_arc = Arc::new(subgraph_provider);

    // Create named subgraph provider for resolving subgraph name->ID mappings
    let subgraph_registrar = Arc::new(SubgraphRegistrar::new(
        &logger_factory,
        link_resolver,
        subgraph_provider_arc.clone(),
        store.clone(),
        store.clone(),
        NODE_ID.clone(),
        SubgraphVersionSwitchingMode::Instant,
    ));

    let subgraph_name = SubgraphName::new(SUBGRAPH_NAME).unwrap();
    tokio::spawn(
        subgraph_registrar
            .create_subgraph(subgraph_name.clone())
            .then(move |_| {
                subgraph_registrar
                    .create_subgraph_version(subgraph_name, SUBGRAPH_ID.clone(), NODE_ID.clone())
                    .then(|result| {
                        result.expect("Failed to create subgraph");
                        Ok(())
                    })
                    .and_then(move |_| subgraph_registrar.start())
            })
            .then(|start_result| {
                start_result.expect("failed to start subgraph");
                Ok(())
            }),
    );

    let mut graphql_server = GraphQLQueryServer::new(
        &logger_factory,
        graphql_runner.clone(),
        store.clone(),
        NODE_ID.clone(),
    );
    let mut subscription_server =
        GraphQLSubscriptionServer::new(&logger, graphql_runner.clone(), store.clone());

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

    future::empty()
}
*/
