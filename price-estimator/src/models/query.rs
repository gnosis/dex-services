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
#[serde(rename_all = "camelCase")]
struct RawQuery {
    atoms: Option<bool>,
    hops: Option<usize>,
    batch_id: Option<BatchId>,
}

impl TryFrom<RawQuery> for QueryParameters {
    type Error = Error;

    fn try_from(raw: RawQuery) -> Result<Self> {
        Ok(QueryParameters {
            unit: match raw.atoms {
                Some(true) => Unit::Atoms,
                Some(false) => Unit::BaseUnits,
                None => bail!("'atoms' or parameter must be specified"),
            },
            hops: raw.hops,
            time: match raw.batch_id {
                Some(batch_id) => EstimationTime::Batch(batch_id),
                None => EstimationTime::Now,
            },
        })
    }
}
