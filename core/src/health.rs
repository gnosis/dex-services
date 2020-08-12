//! Module implementing shared basic health reporting.

use warp::{
    http::{header, Response, StatusCode},
    Filter, Rejection, Reply,
};

/// A `warp` filter for responding to health checks.
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::FutureExt as _;

    #[test]
    fn replies_with_no_content() {
        let response = warp::test::request()
            .method("GET")
            .path("/health")
            .reply(&filter())
            .now_or_never()
            .unwrap();
        assert_eq!(response.status(), 204);

        let response = warp::test::request()
            .method("HEAD")
            .path("/health")
            .reply(&filter())
            .now_or_never()
            .unwrap();
        assert_eq!(response.status(), 204);
    }
}
