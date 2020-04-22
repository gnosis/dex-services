use crate::contracts::Web3;
use anyhow::{Context as _, Result};
use ethcontract::{web3::types::BlockId, H256};
use futures::{compat::Future01CompatExt as _, future::BoxFuture, FutureExt as _};

/// Helper trait to make this functionality mockable for tests.
pub trait BlockTimestamp {
    fn block_timestamp(&mut self, block_hash: H256) -> BoxFuture<Result<u64>>;
}

/// During normal operation this is implemented by Web3.
impl BlockTimestamp for Web3 {
    fn block_timestamp(&mut self, block_hash: H256) -> BoxFuture<Result<u64>> {
        async move {
            let block = self.eth().block(BlockId::Hash(block_hash)).compat().await;
            let block = block
                .with_context(|| format!("failed to get block {}", block_hash))?
                .with_context(|| format!("block {} does not exist", block_hash))?;
            Ok(block.timestamp.low_u64())
        }
        .boxed()
    }
}

/// A cache for the block timestamp which avoids having to query the node in the case where we
/// receive multiple events from the same block in a row.
#[derive(Debug)]
pub struct BlockTimestampCache<T> {
    block_timestamp: T,
    hash: H256,
    timestamp: u64,
}

impl<T> BlockTimestampCache<T> {
    pub fn new(block_timestamp: T) -> Self {
        Self {
            block_timestamp,
            hash: H256::zero(),
            timestamp: 0,
        }
    }
}

impl<T: BlockTimestamp + Send> BlockTimestamp for BlockTimestampCache<T> {
    fn block_timestamp(&mut self, block_hash: H256) -> BoxFuture<Result<u64>> {
        async move {
            if self.hash != block_hash {
                let timestamp = self.block_timestamp.block_timestamp(block_hash).await?;
                self.hash = block_hash;
                self.timestamp = timestamp;
            }
            Ok(self.timestamp)
        }
        .boxed()
    }
}
