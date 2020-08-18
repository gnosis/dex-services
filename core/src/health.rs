//! Module implementing shared basic health reporting.

use crate::{
    http_server::Handler,
    util::{AsyncSleep, AsyncSleeping},
};
use anyhow::Result;
use async_std::task::{self, JoinHandle};
use rouille::{Request, Response};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// Trait for asyncronously notifying health information.
#[cfg_attr(test, mockall::automock)]
pub trait HealthReporting: Send + Sync {
    /// Notify that the service is ready.
    fn notify_ready(&self);
}

/// Implementation sharing health information over an HTTP endpoint.
#[derive(Debug, Default)]
pub struct HttpHealthEndpoint {
    ready: AtomicBool,
}

impl HttpHealthEndpoint {
    /// Creates a new HTTP health enpoint.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if the service is ready, false otherwise.
    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

impl HealthReporting for HttpHealthEndpoint {
    fn notify_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }
}

impl Handler for HttpHealthEndpoint {
    fn handle_request(&self, _: &Request) -> Result<Response> {
        Ok(if self.is_ready() {
            Response::empty_204()
        } else {
            Response::text("service unavailable").with_status_code(503)
        })
    }
}

/// Perform a delayed notification to a health reporting instance.
pub fn delayed_notify_ready(health: Arc<dyn HealthReporting>, delay: Duration) -> JoinHandle<()> {
    delayed_notify_ready_with_sleep(health, delay, AsyncSleep)
}

fn delayed_notify_ready_with_sleep(
    health: Arc<dyn HealthReporting>,
    delay: Duration,
    sleeper: impl AsyncSleeping,
) -> JoinHandle<()> {
    task::spawn(async move {
        sleeper.sleep(delay).await;
        log::info!("service is ready");
        health.notify_ready();
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::MockAsyncSleeping;
    use mockall::predicate::eq;

    #[test]
    fn responds_with_204_when_ready() {
        let health = HttpHealthEndpoint::new();
        health.notify_ready();

        let response = health
            .handle_request(&Request::fake_http(
                "GET",
                "/health/readiness",
                vec![],
                vec![],
            ))
            .unwrap();
        assert_eq!(response.status_code, 204);
    }

    #[test]
    fn responds_with_503_when_not_ready() {
        let health = HttpHealthEndpoint::new();

        let response = health
            .handle_request(&Request::fake_http(
                "GET",
                "/health/readiness",
                vec![],
                vec![],
            ))
            .unwrap();
        assert_eq!(response.status_code, 503);
    }

    #[test]
    fn delays_ready_notification() {
        let mut health = MockHealthReporting::new();
        health.expect_notify_ready().return_once(|| {});

        let duration = Duration::from_secs(42);
        let mut sleeper = MockAsyncSleeping::new();
        sleeper
            .expect_sleep()
            .with(eq(duration))
            .return_once(|_| immediate!(()));

        delayed_notify_ready_with_sleep(Arc::new(health), duration, sleeper);
    }
}
