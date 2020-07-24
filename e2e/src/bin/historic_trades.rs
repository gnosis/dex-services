use anyhow::Result;
use core::{history::Settlement, models::BatchId};
use e2e::cmd;
use pricegraph::{Element, Pricegraph, TokenPair};
use std::path::PathBuf;
use structopt::StructOpt;

const FULLY_FILLED_THREASHOLD: f64 = 0.95;

/// Common options for analyzing historic batch data.
#[derive(Debug, StructOpt)]
#[structopt(
    name = "historic_trades",
    about = "Utility for comparing historic trades to price estimates.",
    rename_all = "kebab"
)]
struct Options {
    /// The events registry file store containing past exchange events.
    #[structopt(long, env = "ORDERBOOK_FILE", parse(from_os_str))]
    orderbook_file: PathBuf,
}

fn main() -> Result<()> {
    let options = Options::from_args();

    cmd::for_each_batch(&options.orderbook_file, |history, batch| {
        let auction_elements = history.auction_elements_for_batch(batch)?;
        let settlement = history.settlement_for_batch(batch);

        let new_orders = auction_elements
            .iter()
            .filter_map(|element| {
                if batch == element.valid.from {
                    Some(element)
                } else {
                    None
                }
            })
            .filter(|order| order.price.numerator != 0 && order.price.denominator != 0)
            .collect::<Vec<_>>();
        for order in new_orders {
            let pricegraph = Pricegraph::new(
                auction_elements
                    .iter()
                    .filter(|element| (element.user, element.id) != (order.user, order.id))
                    .cloned(),
            );

            let limit_price = order.price.numerator as f64 / order.price.denominator as f64;
            let effective_amount =
                pricegraph::num::u256_to_f64(order.balance).min(order.remaining_sell_amount as _);
            let estimated_limit_price =
                pricegraph.estimate_limit_price(order.pair, effective_amount);

            if limit_price > estimated_limit_price.unwrap_or_default() {
                // NOTE: Unreasonable price on order.
                continue;
            }

            match settlement.as_ref().and_then(|settlement| {
                settlement
                    .trades
                    .iter()
                    .find(|trade| (trade.owner, trade.order_id) == (order.user, order.id))
            }) {
                Some(trade) => {
                    let trade_ratio = trade.executed_sell_amount as f64 / effective_amount;
                    if trade_ratio > FULLY_FILLED_THREASHOLD {
                        // NOTE: WOOHOO! Estimate was correct!
                        continue;
                    } else {
                        todo!("report partial trade");
                    }
                }
                None => {
                    let settled_xrate = settlement.as_ref().and_then(|settlement| {

                    });
                    if matches!(settled_xrate, Some(xrate) if xrate < 
                    todo!("report missed trade");
                }
            }
        }

        Ok(())
    })
}

enum Trade {
    FullyMatched,
    Skipped,
}
