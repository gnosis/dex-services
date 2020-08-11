//! Module implementing parsing request query parameters.

use anyhow::{bail, Error, Result};
use core::models::BatchId;
use serde::Deserialize;
use std::convert::TryFrom;

/// Common query parameters shared across all price estimation routes.
#[derive(Clone, Debug, Deserialize)]
#[serde(try_from = "RawQuery")]
pub struct QueryParameters {
    /// The unit of the token amounts.
    pub unit: Unit,
    /// The maximum number of hops (i.e. maximum ring trade length) used by the
    /// `pricegraph` search algorithm.
    pub hops: Option<usize>,
    /// The time to load the orderbook at to perform estimations.
    pub time: EstimationTime,
}

/// Units for token amounts.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    /// Atoms, smallest unit for a token.
    Atoms,
    /// Base units, such that `1.0` is `10.pow(decimals)` atoms.
    BaseUnits,
}

impl Default for Unit {
    fn default() -> Self {
        Unit::BaseUnits
    }
}

/// When to perform a price estimate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EstimationTime {
    /// Estimate with the current open orderbook.
    Now,
    /// Estimate with the finalized orderbook at the specified batch.
    Batch(BatchId),
}

/// Intermediate raw query parameters used for parsing.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawQuery {
    atoms: Option<bool>,
    unit: Option<Unit>,
    hops: Option<usize>,
    batch_id: Option<BatchId>,
}

impl TryFrom<RawQuery> for QueryParameters {
    type Error = Error;

    fn try_from(raw: RawQuery) -> Result<Self> {
        Ok(QueryParameters {
            unit: match (raw.atoms, raw.unit) {
                (Some(true), None) => Unit::Atoms,
                (Some(false), None) => Unit::BaseUnits,
                (None, Some(unit)) => unit,
                (None, None) => Unit::default(),
                _ => bail!("only one of 'atoms' or 'unit' parameters can be specified"),
            },
            hops: raw.hops,
            time: match raw.batch_id {
                Some(batch_id) => EstimationTime::Batch(batch_id),
                None => EstimationTime::Now,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::FutureExt as _;
    use warp::Rejection;

    fn query_params(params: &str) -> Result<QueryParameters, Rejection> {
        warp::test::request()
            .path(&format!("/{}", params))
            .filter(&warp::query::<QueryParameters>())
            .now_or_never()
            .unwrap()
    }

    #[test]
    fn default_query_parameters() {
        let query = query_params("").unwrap();
        assert_eq!(query.unit, Unit::BaseUnits);
        assert_eq!(query.hops, None);
        assert_eq!(query.generation, Generation::Current);
    }

    #[test]
    fn all_query_parameters() {
        let query = query_params("?unit=atoms&hops=42&batchId=1337").unwrap();
        assert_eq!(query.unit, Unit::Atoms);
        assert_eq!(query.hops, Some(42));
        assert_eq!(query.generation, Generation::Batch(1337.into()));
    }

    #[test]
    fn invalid_parameters() {
        assert!(query_params("?unit=invalid").is_err());
        assert!(query_params("?hops=invalid").is_err());
        assert!(query_params("?batch_id=invalid").is_err());
    }

    #[test]
    fn atoms_query_parameter() {
        let query = query_params("?atoms=true").unwrap();
        assert_eq!(query.unit, Unit::Atoms);

        let query = query_params("?atoms=false").unwrap();
        assert_eq!(query.unit, Unit::BaseUnits);
    }

    #[test]
    fn mutually_exclusive_unit_parameter() {
        assert!(query_params("?unit=atoms&atoms=true").is_err());
    }
}
