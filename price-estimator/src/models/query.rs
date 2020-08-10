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
            generation: match raw.batch_id {
                Some(batch_id) => Generation::Batch(batch_id),
                None => Generation::Current,
            },
        })
    }
}
