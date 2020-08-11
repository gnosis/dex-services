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
    /// The generation to load the orderbook at.
    pub generation: Generation,
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
pub enum Generation {
    /// The `Pricegraph` will be contructed from the current state of the
    /// orderbook.
    Current,
    /// The `Pricegraph` will be contructed from the finalized orderbook at the
    /// specified batch. This will be the same orderbook that is provided to the
    /// solver when solving for the specified batch.
    Batch(BatchId),
    /// The `Pricegraph` will be contructed from the events up to, and
    /// including, the specified block.
    Block(u64),
    /// The `Pricegraph` will be contructed from the events that occured up to,
    /// and including, the specified timestamp.
    Timestamp(u64),
}

/// Intermediate raw query parameters used for parsing.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RawQuery {
    atoms: Option<bool>,
    unit: Option<Unit>,
    hops: Option<usize>,
    batch_id: Option<BatchId>,
    block_number: Option<u64>,
    timestamp: Option<u64>,
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
            generation: match (raw.batch_id, raw.block_number, raw.timestamp) {
                (None, None, None) => Generation::Current,
                (Some(batch_id), None, None) => Generation::Batch(batch_id),
                (None, Some(block_number), None) => Generation::Block(block_number),
                (None, None, Some(timestamp)) => Generation::Timestamp(timestamp),
                _ => bail!("only one of 'batchId', 'blockNumber', or 'timestamp' parameters can be specified"),
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
        assert!(query_params("?atoms=invalid").is_err());
        assert!(query_params("?hops=invalid").is_err());
        assert!(query_params("?batch_id=invalid").is_err());
        assert!(query_params("?blockNumber=invalid").is_err());
        assert!(query_params("?timestampe=invalid").is_err());
    }

    #[test]
    fn unknown_parameter() {
        assert!(query_params("?answer=42").is_err());
    }

    #[test]
    fn atoms_query_parameter() {
        let query = query_params("?atoms=true").unwrap();
        assert_eq!(query.unit, Unit::Atoms);

        let query = query_params("?atoms=false").unwrap();
        assert_eq!(query.unit, Unit::BaseUnits);
    }

    #[test]
    fn generation_query_parameters() {
        let query = query_params("?batchId=42").unwrap();
        assert_eq!(query.generation, Generation::Batch(42.into()));

        let query = query_params("?blockNumber=123").unwrap();
        assert_eq!(query.generation, Generation::Block(123));

        let query = query_params("?timestamp=1337").unwrap();
        assert_eq!(query.generation, Generation::Timestamp(1337));
    }

    #[test]
    fn mutually_exclusive_unit_parameter() {
        assert!(query_params("?unit=atoms&atoms=true").is_err());
    }

    #[test]
    fn mutually_exclusive_generation_parameter() {
        assert!(query_params("?batchId=123&blockNumber=456").is_err());
        assert!(query_params("?blockNumber=123&timestamp=456").is_err());
        assert!(query_params("?timestamp=123&batchId=456").is_err());
    }
}
