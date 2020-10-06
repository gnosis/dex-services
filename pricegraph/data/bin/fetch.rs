use anyhow::Result;
use contracts::{
    ethcontract::{Address, Web3},
    BatchExchange, BatchExchangeViewer,
};
use env_logger::Env;
use ethcontract::Http;
use std::{
    env,
    fs::File,
    io::{BufWriter, Write},
    path::Path,
};

const ALL_TOKENS: &[Address] = &[];
const CONFIRMATIONS: u64 = 6;
const PAGE_SIZE: u16 = 50;

fn main() {
    env_logger::init_from_env(Env::default().default_filter_or("warn,fetch=debug"));

    if let Err(err) = futures::executor::block_on(run()) {
        log::error!("Error retrieving orderbook: {:?}", err);
        std::process::exit(-1);
    }
}

async fn run() -> Result<()> {
    let url = format!(
        "https://mainnet.infura.io/v3/{}",
        env::var("INFURA_PROJECT_ID")?,
    );
    let http = Http::new(&url)?;
    let web3 = Web3::new(http);

    let exchange = BatchExchange::deployed(&web3).await?;
    let viewer = BatchExchangeViewer::deployed(&web3).await?;

    let block_number = {
        let latest_block = web3.eth().block_number().await?;
        latest_block - CONFIRMATIONS
    };

    let batch_id = {
        let current_batch_id = exchange
            .get_current_batch_id()
            .block(block_number.into())
            .call()
            .await?;
        current_batch_id - 1
    };
    let mut output = File::create(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../orderbook-{}.hex", batch_id)),
    )?;

    log::info!(
        "retrieving orderbook at block {} until batch {}",
        block_number,
        batch_id,
    );

    let mut previous_page_user = Address::default();
    let mut previous_page_user_offset = 0;
    while {
        log::debug!(
            "retrieving page {}-{}",
            previous_page_user,
            previous_page_user_offset
        );

        let (elements, has_next_page, next_page_user, next_page_user_offset) = viewer
            .get_finalized_order_book_paginated(
                ALL_TOKENS.into(),
                previous_page_user,
                previous_page_user_offset,
                PAGE_SIZE,
            )
            .block(block_number.into())
            .call()
            .await?;
        write_elements(&mut output, &elements)?;

        previous_page_user = next_page_user;
        previous_page_user_offset = next_page_user_offset;
        has_next_page
    } {}

    Ok(())
}

fn write_elements(mut output: impl Write, elements: &[u8]) -> Result<()> {
    const ORDER_STRIDE: usize = 114;
    const SECTIONS: &[usize] = &[20, 32, 2, 2, 4, 4, 16, 16, 16, 2];

    assert_eq!(elements.len() % ORDER_STRIDE, 0);
    assert_eq!(ORDER_STRIDE, SECTIONS.iter().sum::<usize>());

    let mut writer = BufWriter::new(&mut output);
    for element in elements.chunks(ORDER_STRIDE) {
        let encoded = SECTIONS
            .iter()
            .scan(element, |remaining, &section| {
                let (bytes, rest) = remaining.split_at(section);
                *remaining = rest;
                Some(hex::encode(bytes))
            })
            .collect::<Vec<_>>()
            .join(" ");

        writer.write_all(encoded.as_bytes())?;
        writer.write_all(b"\n")?;
    }

    Ok(())
}
