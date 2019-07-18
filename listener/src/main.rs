extern crate env_logger;
extern crate graph;
extern crate graph_core;
extern crate graph_datasource_ethereum;
extern crate lazy_static;
#[macro_use]
extern crate slog;

mod runtime_host;
mod link_resolver;

use lazy_static::lazy_static;
use std::env;
use std::sync::Arc;
use std::time::Duration;

use graph::components::forward;
use graph::prelude::{
    SubgraphRegistrar as SubgraphRegistrarTrait,
    *
};
use graph::log::logger;
use graph::tokio_executor;
use graph::tokio_timer;
use graph::tokio_timer::timer::Timer;

use graph_core::{SubgraphInstanceManager, SubgraphRegistrar, SubgraphAssignmentProvider};

use graph_datasource_ethereum::{BlockStreamBuilder, Transport};

use graph_server_http::GraphQLServer as GraphQLQueryServer;
use graph_server_websocket::SubscriptionServer as GraphQLSubscriptionServer;

use graph_store_postgres::{Store as DieselStore, StoreConfig};

use runtime_host::RustRuntimeHost;
use link_resolver::LocalLinkResolver;

lazy_static! {
    static ref ANCESTOR_COUNT: u64 = 50;
    static ref REORG_THRESHOLD: u64 = 50;
    static ref BLOCK_POLLING_INTERVAL: Duration = Duration::from_millis(1000);
    
    static ref NODE_ID: NodeId = NodeId::new("default").unwrap();
    static ref SUBGRAPH_NAME: SubgraphName = SubgraphName::new("dfusion").unwrap();
    static ref SUBGRAPH_ID: SubgraphDeploymentId = SubgraphDeploymentId::new("dfusion").unwrap();

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
            .core_threads(100)
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
                .block_on(future::lazy(async_main))
                .expect("Failed to run main function");
        })
    });
}

fn async_main() -> impl Future<Item = (), Error = ()> + Send + 'static {
    env_logger::init();

    let postgres_url = env::var("POSTGRES_URL").expect("Specify POSTGRES_URL variable");
    let ethereum_node_url = env::var("ETHEREUM_NODE_URL").expect("Specify ETHEREUM_RPC variable");
    let network_name = env::var("NETWORK_NAME").expect("Specify NETWORK_NAME variable");

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
        0,
    ));
    let eth_net_identifiers = match eth_adapter.net_identifiers(&logger).wait() {
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

    let store = Arc::new(DieselStore::new(
        StoreConfig {
            postgres_url,
            network_name: network_name.clone(),
            start_block: 0,
        },
        &logger,
        eth_net_identifiers,
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

    let subgraph_instance_manager = SubgraphInstanceManager::new(
        &logger_factory,
        store.clone(),
        RustRuntimeHost {},
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
        SubgraphVersionSwitchingMode::Instant
    ));

    tokio::spawn(	
        subgraph_registrar
            .create_subgraph(SUBGRAPH_NAME.clone())
            .then( move |_| {
                subgraph_registrar.create_subgraph_version(
                    SUBGRAPH_NAME.clone(), SUBGRAPH_ID.clone(), NODE_ID.clone()
                )
                .then(|result| {	
                    Ok(result.expect("Failed to create subgraph"))
                })
                .and_then(move |_| {
                    subgraph_registrar.start()
                })
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