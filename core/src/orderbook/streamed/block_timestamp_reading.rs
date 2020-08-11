use crate::contracts::Web3;
use crate::transport::HttpTransport;
use anyhow::{Context as _, Result};
use ethcontract::web3::transports::Batch;
use ethcontract::{
    web3::types::{Block, BlockId},
    H256,
};
use futures::{compat::Future01CompatExt as _, future::BoxFuture, FutureExt as _};
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;

/// Helper trait to make this functionality mockable for tests.
pub trait BlockTimestampReading {
    fn block_timestamp(&mut self, block: BlockId) -> BoxFuture<Result<u64>>;
}

pub trait BlockTimestampBatchReading {
    fn block_timestamps(
        &mut self,
        block_hashes: HashSet<H256>,
        block_batch_size: usize,
    ) -> BoxFuture<Result<Vec<(H256, u64)>>>;
}

/// During normal operation this is implemented by Web3.
impl BlockTimestampReading for Web3 {
    fn block_timestamp(&mut self, block: BlockId) -> BoxFuture<Result<u64>> {
        async move {
            let block_header = self.eth().block(block.clone()).compat().await;
            let block = block_header
                .with_context(|| format!("failed to get block {:?}", block))?
                .with_context(|| format!("block {:?} does not exist", block))?;
            Ok(block.timestamp.low_u64())
        }
        .boxed()
    }
}

impl BlockTimestampBatchReading for Web3 {
    fn block_timestamps(
        &mut self,
        block_hashes: HashSet<H256>,
        block_batch_size: usize,
    ) -> BoxFuture<Result<Vec<(H256, u64)>>> {
        let batched_web3 = ethcontract::web3::Web3::new(Batch::new(self.transport().clone()));
        async move {
            let mut result = Vec::with_capacity(block_hashes.len());
            for chunk in Vec::from_iter(block_hashes.into_iter()).chunks(block_batch_size) {
                let partial_result = query_block_timestamps_batched(&batched_web3, chunk).await?;
                result.extend(partial_result);
            }
            Ok(result)
        }
        .boxed()
    }
}

type BatchedWeb3 = ethcontract::web3::Web3<Batch<HttpTransport>>;
async fn query_block_timestamps_batched(
    batched_web3: &BatchedWeb3,
    block_hashes: &[H256],
) -> Result<Vec<(H256, u64)>> {
    block_hashes.iter().for_each(|hash| {
        batched_web3.eth().block(BlockId::Hash(*hash));
    });
    let result = batched_web3.transport().submit_batch().compat().await;
    result
        .with_context(|| "Batch RPC call to get block hashes failed")?
        .into_iter()
        .map(|response| {
            let block: Block<H256> = serde_json::from_value(response?)?;
            Ok((
                block
                    .hash
                    .expect("blocks queried by hash should contain a hash"),
                block.timestamp.low_u64(),
            ))
        })
        .collect()
}

/// A cache for the block timestamp.
#[derive(Debug)]
pub struct CachedBlockTimestampReader<T> {
    inner: T,
    cache: HashMap<H256, u64>,
}

impl<T: BlockTimestampBatchReading + Send> CachedBlockTimestampReader<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            cache: HashMap::new(),
        }
    }

    pub fn prepare_cache(
        &mut self,
        block_hashes: HashSet<H256>,
        block_batch_size: usize,
    ) -> BoxFuture<Result<()>> {
        let missing_hashes = block_hashes
            .into_iter()
            .filter(|hash| !self.cache.contains_key(&hash))
            .collect();
        async move {
            self.cache.extend(
                self.inner
                    .block_timestamps(missing_hashes, block_batch_size)
                    .await?,
            );
            Ok(())
        }
        .boxed()
    }
}

impl<T: BlockTimestampReading + Send> BlockTimestampReading for CachedBlockTimestampReader<T> {
    fn block_timestamp(&mut self, block: BlockId) -> BoxFuture<Result<u64>> {
        async move {
            let block_hash = match block {
                BlockId::Hash(hash) => hash,
                _ => return self.inner.block_timestamp(block).await,
            };

            if let Some(timestamp) = self.cache.get(&block_hash) {
                Ok(*timestamp)
            } else {
                let timestamp = self.inner.block_timestamp(block_hash.into()).await?;
                self.cache.insert(block_hash, timestamp);
                Ok(timestamp)
            }
        }
        .boxed()
    }
}
