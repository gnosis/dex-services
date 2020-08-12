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
    warp::path!("health" / "readiness")
        .and(warp::get().or(warp::head()))
        .map(|_| {
            Response::builder()
                .status(StatusCode::NO_CONTENT)
                .header(header::CACHE_CONTROL, "no-store")
                .body("")
        })
}

/// Trait for asyncronously notifying health information.
#[cfg_attr(test, mockall::automock)]
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
        Self::with_server(WarpServer(bind))
    }

    fn with_server(server: impl Serving) -> Self {
        // NOTE: We are using `futures` synchronization primitives instead of
        // `async_std` because they work regardless of the runtime. This allows
        // our channel's sender to be driven by `async_std`, since that is what
        // we are using in `core` and the `driver`, but the receiver to be
        // driven by `tokio`, which is required by `warp`.

        let (sender, receiver) = mpsc::channel(0);
        thread::spawn(move || {
            if let Err(err) = server.serve(receiver) {
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

/// Internal trait responsible for serving the HTTP endpoint on a specified
/// address.
#[cfg_attr(test, mockall::automock)]
trait Serving: Send + 'static {
    /// Serve an endpoint once a message is received on the channel and as long
    /// as the channel is open.
    fn serve(self, receiver: Receiver<()>) -> Result<()>;
}

/// A warp implementation of the `Serving` trait.
struct WarpServer(SocketAddr);

impl Serving for WarpServer {
    fn serve(self, mut receiver: Receiver<()>) -> Result<()> {
        let Self(bind) = self;

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::FutureWaitExt as _;
    use anyhow::ensure;
    use futures::channel::oneshot;
    use std::{
        mem,
        sync::{atomic::AtomicUsize, Arc},
        time::Duration,
    };

    #[test]
    fn replies_with_no_content() {
        let response = warp::test::request()
            .method("GET")
            .path("/health/readiness")
            .reply(&filter())
            .now_or_never()
            .unwrap();
        assert_eq!(response.status(), 204);

        let response = warp::test::request()
            .method("HEAD")
            .path("/health/readiness")
            .reply(&filter())
            .now_or_never()
            .unwrap();
        assert_eq!(response.status(), 204);
    }

    #[test]
    fn sends_single_signal() {
        let signals = Arc::new(AtomicUsize::new(0));
        let (done_tx, done_rx) = oneshot::channel();
        let mut server = MockServing::new();
        server.expect_serve().return_once({
            let signals = signals.clone();
            move |receiver| {
                futures::executor::block_on(async move {
                    let count = receiver.collect::<Vec<_>>().await.len();
                    signals.store(count, Ordering::SeqCst);
                    done_tx.send(()).unwrap();
                    Ok(())
                })
            }
        });

        let endpoint = HttpHealthEndpoint::with_server(server);
        futures::future::try_join3(
            endpoint.notify_ready(),
            endpoint.notify_ready(),
            endpoint.notify_ready(),
        )
        .wait()
        .unwrap();
        endpoint.notify_ready().wait().unwrap();
        endpoint.notify_ready().wait().unwrap();
        mem::drop(endpoint);

        done_rx.wait().unwrap();
        assert_eq!(signals.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn warp_server_stops_when_channel_closes() {
        let (mut sender, receiver) = mpsc::channel(0);
        let serving =
            thread::spawn(move || WarpServer("0.0.0.0:0".parse().unwrap()).serve(receiver));

        sender.send(()).wait().unwrap();
        drop(sender);

        assert!(serving.join().unwrap().is_ok());
    }

    #[test]
    #[ignore]
    // Run with `cargo test warp_hosts_endpoint -- --ignored`.
    fn warp_hosts_endpoint() {
        let address = "0.0.0.0:1337";
        let (mut sender, receiver) = mpsc::channel(0);
        thread::spawn(move || {
            WarpServer(address.parse().unwrap())
                .serve(receiver)
                .unwrap()
        });

        let check_health = || -> Result<()> {
            let response = isahc::get(format!("http://{}/health", address))?;
            ensure!(
                response.status() == 204,
                "invalid HTTP response {:?}",
                response,
            );

            Ok(())
        };

        assert!(check_health().is_err());

        // NOTE: Waiting for `warp` to start up is inheritely racy, so just
        // sleep a short amount and then check the health endpoint.
        sender.send(()).wait().unwrap();
        thread::sleep(Duration::from_millis(100));

        assert!(check_health().is_ok());
    }
}
