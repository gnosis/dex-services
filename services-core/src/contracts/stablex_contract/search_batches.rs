use anyhow::{anyhow, Result};
use ethcontract::{prelude::Web3, transport::DynTransport, web3::types::Block, BlockNumber, H256};

fn get_block_batch_id<T>(block: &Block<T>) -> u32 {
    const BATCH_DURATION: u64 = 300;
    (block.timestamp.as_u64() / BATCH_DURATION) as _
}

async fn get_block(
    web3: &Web3<DynTransport>,
    block_number: BlockNumber,
) -> Result<ethcontract::web3::types::Block<H256>> {
    web3.eth()
        .block(block_number.into())
        .await?
        .ok_or_else(|| anyhow!("block {:?} is missing", block_number))
}

struct Bounds {
    lower: u64,
    upper: u64,
}
impl Bounds {
    fn diff(&self) -> u64 {
        self.upper - self.lower
    }
    fn mid(&self) -> u64 {
        (self.upper + self.lower) / 2
    }
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait BatchIdRetrieving {
    /// Return the id of the unique batch associated with the given block
    async fn batch_id_from_block(&self, block_number: BlockNumber) -> Result<u32>;

    /// Return a pair containing the id of the batch we are currently in and the last block number, in this order
    async fn current_batch_id_and_block_number(&self) -> Result<(u32, u64)>;
}

#[async_trait::async_trait]
impl BatchIdRetrieving for Web3<DynTransport> {
    async fn batch_id_from_block(&self, block_number: BlockNumber) -> Result<u32> {
        let current_block = get_block(&self, block_number).await?;
        Ok(get_block_batch_id(&current_block))
    }

    async fn current_batch_id_and_block_number(&self) -> Result<(u32, u64)> {
        let current_block = get_block(&self, BlockNumber::Latest).await?;
        let batch_id = get_block_batch_id(&current_block);
        let block_number = current_block
            .number
            .ok_or_else(|| anyhow!("latest block {:?} has no block number", current_block.hash))?
            .as_u64();
        Ok((batch_id, block_number))
    }
}

pub async fn search_last_block_for_batch(
    batch_id_retrieving: &impl BatchIdRetrieving,
    batch_id: u32,
) -> Result<u64> {
    let (current_batch_id, current_block_number) = batch_id_retrieving
        .current_batch_id_and_block_number()
        .await?;

    // find lower bound for binary search
    let mut step = 1_u64;
    let mut bounds = Bounds {
        lower: current_block_number,
        upper: current_block_number,
    };
    let mut lower_batch_id = current_batch_id;
    while batch_id < lower_batch_id {
        bounds.upper = bounds.lower;
        if step >= bounds.lower {
            bounds.lower = 0;
            break;
        } else {
            bounds.lower -= step;
            lower_batch_id = batch_id_retrieving
                .batch_id_from_block(bounds.lower.into())
                .await?;
        }
        step *= 2;
    }

    // find last block for batch within bounds
    while bounds.diff() > 1 {
        let mid = bounds.mid();
        let mid_batch_id = batch_id_retrieving.batch_id_from_block(mid.into()).await?;
        if batch_id >= mid_batch_id {
            bounds.lower = mid;
        } else {
            bounds.upper = mid;
        }
    }
    Ok(bounds.lower)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use futures::FutureExt as _;

    #[test]
    fn incremental_binary_search() {
        //                                   2        5     7     9  10
        let batch_ids: Vec<u32> = vec![1, 1, 1, 2, 2, 2, 3, 3, 5, 5, 6];
        let mut mock_batch_id_retrieving = MockBatchIdRetrieving::new();
        mock_batch_id_retrieving
            .expect_batch_id_from_block()
            .withf(|block_number: &BlockNumber| matches!(block_number, BlockNumber::Number(_)))
            .returning({
                let batch_ids = batch_ids.clone();
                move |block_number: BlockNumber| {
                    let index = if let BlockNumber::Number(n) = block_number {
                        n
                    } else {
                        panic!("Not implemented");
                    }
                    .as_u64() as usize;
                    let batch_id = batch_ids[index];
                    Ok(batch_id)
                }
            });

        mock_batch_id_retrieving
            .expect_current_batch_id_and_block_number()
            .returning(move || {
                let latest_block = batch_ids.len() as u64 - 1;
                let latest_batch_id = batch_ids[latest_block as usize];
                Ok((latest_batch_id, latest_block))
            });

        assert_eq!(
            search_last_block_for_batch(&mock_batch_id_retrieving, 1)
                .now_or_never()
                .unwrap()
                .unwrap(),
            2
        );
        assert_eq!(
            search_last_block_for_batch(&mock_batch_id_retrieving, 2)
                .now_or_never()
                .unwrap()
                .unwrap(),
            5
        );
        assert_eq!(
            search_last_block_for_batch(&mock_batch_id_retrieving, 3)
                .now_or_never()
                .unwrap()
                .unwrap(),
            7
        );
        assert_eq!(
            search_last_block_for_batch(&mock_batch_id_retrieving, 4)
                .now_or_never()
                .unwrap()
                .unwrap(),
            7
        ); // note: no error if batch does not exist
        assert_eq!(
            search_last_block_for_batch(&mock_batch_id_retrieving, 5)
                .now_or_never()
                .unwrap()
                .unwrap(),
            9
        );
        assert_eq!(
            search_last_block_for_batch(&mock_batch_id_retrieving, 6)
                .now_or_never()
                .unwrap()
                .unwrap(),
            10
        );
        assert_eq!(
            search_last_block_for_batch(&mock_batch_id_retrieving, 7)
                .now_or_never()
                .unwrap()
                .unwrap(),
            10
        ); // note: returns last batch for batches in the future
    }
}
