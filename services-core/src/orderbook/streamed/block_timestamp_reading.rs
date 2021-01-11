use crate::{contracts::Web3, transport::HttpTransport};
use anyhow::{Context as _, Result};
use ethcontract::{
    web3::{
        transports::Batch,
        types::{Block, BlockId, BlockNumber},
    },
    H256,
};
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
};

/// Helper trait to make this functionality mockable for tests.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait BlockTimestampReading: Send + Sync {
    async fn block_timestamp(&mut self, block_id: BlockId) -> Result<u64>;
}

/// During normal operation this is implemented by Web3.
#[async_trait::async_trait]
impl BlockTimestampReading for Web3 {
    async fn block_timestamp(&mut self, block_id: BlockId) -> Result<u64> {
        let block = self.eth().block(block_id).await;
        let block = block
            .with_context(|| format!("failed to get block {:?}", block_id))?
            .with_context(|| format!("block {:?} does not exist", block_id))?;
        Ok(block.timestamp.low_u64())
    }
}

pub type BlockPair = (H256, Block<H256>);

#[async_trait::async_trait]
pub trait BatchedBlockReading: Send + Sync {
    async fn blocks(
        &mut self,
        block_hashes: HashSet<H256>,
        block_batch_size: usize,
    ) -> Result<Vec<BlockPair>>;
}

#[async_trait::async_trait]
impl BatchedBlockReading for Web3 {
    async fn blocks(
        &mut self,
        block_hashes: HashSet<H256>,
        block_batch_size: usize,
    ) -> Result<Vec<BlockPair>> {
        let batched_web3 = ethcontract::web3::Web3::new(Batch::new(self.transport().clone()));
        let mut result = Vec::with_capacity(block_hashes.len());
        for chunk in block_hashes
            .into_iter()
            .collect::<Vec<_>>()
            .chunks(block_batch_size)
        {
            let partial_result = query_block_timestamps_batched(&batched_web3, chunk).await?;
            result.extend(partial_result);
        }
        Ok(result)
    }
}

type BatchedWeb3 = ethcontract::web3::Web3<Batch<HttpTransport>>;

async fn query_block_timestamps_batched(
    batched_web3: &BatchedWeb3,
    block_hashes: &[H256],
) -> Result<Vec<BlockPair>> {
    block_hashes.iter().for_each(|hash| {
        batched_web3.eth().block(BlockId::Hash(*hash));
    });
    let result = batched_web3.transport().submit_batch().await;
    result
        .with_context(|| "Batch RPC call to get block hashes failed")?
        .into_iter()
        .map(|response| {
            let block: Block<H256> = serde_json::from_value(response?)?;
            Ok((
                block
                    .hash
                    .expect("blocks queried by hash should contain a hash"),
                block,
            ))
        })
        .collect()
}

/// A block cache ID. This is different than a `BlockId` in that it can't take
/// special values like `latest` and `earliest` and it implements `Hash` so it
/// can be use to index into a `HashMap`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum BlockCacheId {
    Hash(H256),
    Number(u64),
}

impl TryFrom<BlockId> for BlockCacheId {
    type Error = BlockId;

    fn try_from(block_id: BlockId) -> Result<Self, Self::Error> {
        match block_id {
            BlockId::Hash(hash) => Ok(BlockCacheId::Hash(hash)),
            BlockId::Number(BlockNumber::Number(number)) => {
                Ok(BlockCacheId::Number(number.as_u64()))
            }
            _ => Err(block_id),
        }
    }
}

impl Into<BlockId> for BlockCacheId {
    fn into(self) -> BlockId {
        match self {
            BlockCacheId::Hash(hash) => BlockId::Hash(hash),
            BlockCacheId::Number(number) => BlockId::Number(number.into()),
        }
    }
}

/// A cache for the block timestamp.
#[derive(Debug)]
pub struct CachedBlockTimestampReader<T> {
    inner: T,
    latest_block: u64,
    confirmation_count: u64,
    cache: HashMap<BlockCacheId, u64>,
}

impl<T> CachedBlockTimestampReader<T> {
    pub fn new(inner: T, confirmation_count: u64) -> Self {
        Self {
            inner,
            latest_block: 0,
            confirmation_count,
            cache: HashMap::new(),
        }
    }

    pub fn update_latest_block(&mut self, latest_block_number: u64) {
        self.latest_block = latest_block_number;
    }

    fn is_cacheable(&self, block: BlockCacheId) -> bool {
        match block {
            BlockCacheId::Hash(_) => true,
            BlockCacheId::Number(block_number) => {
                let confirmed_block = self.latest_block.saturating_sub(self.confirmation_count);
                block_number <= confirmed_block
            }
        }
    }

    fn cache(&mut self, block: BlockCacheId, timestamp: u64) {
        if self.is_cacheable(block) {
            self.cache.insert(block, timestamp);
        }
    }
}

impl<T: BatchedBlockReading> CachedBlockTimestampReader<T> {
    pub async fn prepare_cache(
        &mut self,
        block_hashes: HashSet<H256>,
        block_batch_size: usize,
        latest_block_number: u64,
    ) -> Result<()> {
        let missing_hashes = block_hashes
            .into_iter()
            .filter(|hash| !self.cache.contains_key(&BlockCacheId::Hash(*hash)))
            .collect();
        self.update_latest_block(latest_block_number);
        let blocks = self.inner.blocks(missing_hashes, block_batch_size).await?;
        for (hash, block) in blocks {
            let timestamp = block.timestamp.as_u64();
            self.cache(BlockCacheId::Hash(hash), timestamp);
            if let Some(block_number) = block.number {
                self.cache(BlockCacheId::Number(block_number.as_u64()), timestamp);
            }
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl<T: BlockTimestampReading> BlockTimestampReading for CachedBlockTimestampReader<T> {
    async fn block_timestamp(&mut self, block_id: BlockId) -> Result<u64> {
        let block = match BlockCacheId::try_from(block_id) {
            Ok(block) => block,
            Err(block_id) => return self.inner.block_timestamp(block_id).await,
        };

        if let Some(timestamp) = self.cache.get(&block) {
            Ok(*timestamp)
        } else {
            let timestamp = self.inner.block_timestamp(block.into()).await?;
            self.cache(block, timestamp);
            Ok(timestamp)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt as _;
    use mockall::{predicate::eq, Sequence};

    #[test]
    fn caches_block_timestamps_by_hash() {
        let hash = H256::repeat_byte(42);
        let mut inner = MockBlockTimestampReading::new();
        inner
            .expect_block_timestamp()
            .with(eq(BlockId::Hash(hash)))
            .return_once(|_| Ok(1337));

        let mut block_timestamp_reading = CachedBlockTimestampReader::new(inner, 0);

        assert_eq!(
            block_timestamp_reading
                .block_timestamp(hash.into())
                .now_or_never()
                .unwrap()
                .unwrap(),
            1337
        );
        assert_eq!(
            block_timestamp_reading
                .block_timestamp(hash.into())
                .now_or_never()
                .unwrap()
                .unwrap(),
            1337
        );
    }

    #[test]
    fn caches_confirmed_block_number_timestamps() {
        let mut inner = MockBlockTimestampReading::new();
        inner
            .expect_block_timestamp()
            .with(eq(BlockId::Number(41.into())))
            .return_once(|_| Ok(1000));
        let mut seq = Sequence::new();
        inner
            .expect_block_timestamp()
            .times(1)
            .in_sequence(&mut seq)
            .with(eq(BlockId::Number(42.into())))
            .returning(|_| Ok(1337));
        inner
            .expect_block_timestamp()
            .times(1)
            .in_sequence(&mut seq)
            .with(eq(BlockId::Number(42.into())))
            .returning(|_| Ok(1338));

        let mut block_timestamp_reading = CachedBlockTimestampReader::new(inner, 2);
        block_timestamp_reading.update_latest_block(43);

        let mut block_timestamp = move |block: u64| {
            block_timestamp_reading
                .block_timestamp(BlockId::Number(block.into()))
                .now_or_never()
                .unwrap()
                .unwrap()
        };

        assert_eq!(block_timestamp(41), 1000);
        assert_eq!(block_timestamp(41), 1000);
        assert_eq!(block_timestamp(42), 1337);
        assert_eq!(block_timestamp(42), 1338);
    }
}
