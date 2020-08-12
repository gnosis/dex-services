//! Module implementing shared basic health reporting.

use anyhow::{anyhow, bail, Result};
use futures::{
    channel::mpsc::{self, Receiver, Sender},
    future::{BoxFuture, FutureExt as _},
    lock::Mutex,
    sink::SinkExt as _,
    stream::StreamExt as _,
};
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};
use tokio::runtime::Runtime;
use warp::{
    http::{header, Response, StatusCode},
    Filter, Rejection, Reply,
};

/// A `warp` filter for responding to health checks. This filter can be reused
/// in cases where a `warp` serving task already exists.
pub fn filter() -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::path!("health")
        .and(warp::get().or(warp::head()))
        .map(|_| {
            Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header(header::CACHE_CONTROL, "no-store")
                .body("")
        })
}

/// Trait for asyncronously notifying health information.
pub trait HealthReporting: Send + Sync {
    /// Notify that the service is ready.
    fn notify_ready<'a>(&'a self) -> BoxFuture<'a, Result<()>>;
}

/// Implementation sharing health information over an HTTP endpoint.
pub struct HttpHealthEndpoint {
    ready: AtomicBool,
    sender: Mutex<Sender<()>>,
}

impl HttpHealthEndpoint {
    /// Creates a new HTTP health endpoint.
    pub fn new(bind: SocketAddr) -> Self {
        // NOTE: We are using `futures` synchronization primitives instead of
        // `async_std` because they work regardless of the runtime. This allows
        // our channel's sender to be driven by `async_std`, since that is what
        // we are using in `core` and the `driver`, but the receiver to be
        // driven by `tokio`, which is required by `warp`.

        let (sender, receiver) = mpsc::channel(0);
        thread::spawn(move || {
            if let Err(err) = serve(bind, receiver) {
                log::error!("error in health endpoint background thread: {:?}", err);
            }
        });

        Self {
            ready: AtomicBool::new(false),
            sender: Mutex::new(sender),
        }
    }
}

impl HealthReporting for HttpHealthEndpoint {
    fn notify_ready<'a>(&'a self) -> BoxFuture<'a, Result<()>> {
        async move {
            let previously_ready = self.ready.swap(true, Ordering::SeqCst);
            if !previously_ready {
                self.sender
                    .lock()
                    .await
                    .send(())
                    .await
                    .map_err(|_| anyhow!("health endpoint thread unexpectedly stopped"))?;
            }
            Ok(())
        }
        .boxed()
    }
}

fn serve(bind: SocketAddr, mut receiver: Receiver<()>) -> Result<()> {
    // NOTE: `warp` requires a `tokio` runtime, so start one up.
    Runtime::new()?.block_on(async move {
        // NOTE: Wait for the health reporting signal to come.
        if receiver.next().await.is_none() {
            return Ok(());
        }

        let filter = filter().with(warp::log("core::health"));
        let mut serve = warp::serve(filter).run(bind).boxed().fuse();
        futures::select! {
            _ = serve => bail!("warp server unexpectedly stopped"),
            _ = receiver.next() => Ok(()),
        }
    })
}
