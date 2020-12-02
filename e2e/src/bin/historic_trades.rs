use anyhow::Result;
use contracts::batch_exchange::event_data::Trade;
use e2e::cmd::{self, Reporting};
use pricegraph::{Element, Pricegraph, TokenPair, FEE_FACTOR, U256};
use services_core::{history::Settlement, models::BatchId};
use std::{fs::File, io::Write, path::PathBuf};
use structopt::StructOpt;

const FULLY_FILLED_THRESHOLD: f64 = 0.95;
const MIN_AMOUNT: u128 = pricegraph::MIN_AMOUNT as _;

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

    /// The output directory for the computed results.
    #[structopt(long, env = "OUTPUT_DIR", default_value = "target", parse(from_os_str))]
    output_dir: PathBuf,
}

fn main() -> Result<()> {
    let options = Options::from_args();

    let mut report = Report::new(File::create(options.output_dir.join("trades.csv"))?);
    report.header()?;

    cmd::for_each_batch(
        &options.orderbook_file,
        report,
        |samples, history, batch| {
            let auction_elements = history.auction_elements_for_batch(batch)?;
            let settlement = history.settlement_for_batch(batch);

            let new_orders = auction_elements
                .iter()
                .filter(|order| batch == order.valid.from)
                .filter(|order| order.price.numerator != 0 && order.price.denominator > MIN_AMOUNT)
                .filter(|order| order.balance > U256::from(MIN_AMOUNT));
            for order in new_orders {
                let settlement = settlement.as_ref();
                let pricegraph = Pricegraph::new(
                    auction_elements
                        .iter()
                        .filter(|element| (element.user, element.id) != (order.user, order.id))
                        .cloned(),
                );

                let meta = OrderMetadata::compute(settlement, order, &pricegraph)?;
                let result = if meta.is_reasonably_priced_order() {
                    process_reasonable_order(&meta)
                } else {
                    process_unreasonable_order(&meta)
                };

                samples.record_sample(Row {
                    batch,
                    solved: settlement.is_some(),
                    order_uid: format!("{}-{}", order.user, order.id),
                    meta,
                    result,
                })?;
            }

            Ok(())
        },
    )
}

fn process_reasonable_order(meta: &OrderMetadata) -> TradeResult {
    match meta.fill_ratio() {
        Some(trade_ratio) if trade_ratio > FULLY_FILLED_THRESHOLD => TradeResult::FullyMatched,
        Some(_) => TradeResult::PartiallyMatched,
        None => match meta.settled_xrate {
            Some(xrate) if xrate > meta.limit_price => TradeResult::SkippedMatchableOrder,
            Some(_) => TradeResult::OverlyOptimistic,
            None => TradeResult::NotTraded,
        },
    }
}

fn process_unreasonable_order(meta: &OrderMetadata) -> TradeResult {
    match meta.settled_xrate {
        Some(xrate) if xrate > meta.limit_price => TradeResult::OverlyPessimistic,
        _ => TradeResult::UnreasonableOrderNotMatched,
    }
}

struct OrderMetadata {
    effective_sell_amount: f64,
    limit_price: f64,
    estimated_limit_price: Option<f64>,
    settled_xrate: Option<f64>,
    trade: Option<Trade>,
}

impl OrderMetadata {
    /// Computes order metadata based on a batch settlement (solution), order
    /// data and a `Pricegraph` instance.
    fn compute(
        settlement: Option<&Settlement>,
        order: &Element,
        pricegraph: &Pricegraph,
    ) -> Result<Self> {
        let effective_sell_amount = order
            .balance
            .to_f64_lossy()
            .min(order.remaining_sell_amount as _);
        let limit_price = order.price.numerator as f64 / order.price.denominator as f64;
        let estimated_limit_price = pricegraph
            .estimate_limit_price(order.pair.into_unbounded_range(), effective_sell_amount)?;

        // NOTE: Compare the settled exchange rate to the limit price, this is
        // because the limit price must be respected by the actual executed
        // exchange rate.
        let settled_xrate =
            settlement.and_then(|settlement| find_settled_exchange_rate(settlement, order.pair));
        let trade = settlement.and_then(|settlement| find_trade(settlement, order));

        #[cfg(debug_assertions)]
        {
            if let (Some(xrate), Some(trade)) = (settled_xrate, trade.as_ref()) {
                // NOTE: check that the settled exchange rate matches the trade
                // exchange rate.
                let trade_xrate =
                    trade.executed_buy_amount as f64 / trade.executed_sell_amount as f64;

                // NOTE: Assert that the error between settled xrate and traded
                // xrate is significantly smaller than the fees, so make sure
                // that they were added in the correct direction.
                assert!((xrate - trade_xrate).abs() / xrate < 0.0001);
            }
        }

        Ok(OrderMetadata {
            effective_sell_amount,
            limit_price,
            estimated_limit_price,
            settled_xrate,
            trade,
        })
    }

    /// Returns `true` if an order is considered reasonably priced, `false`
    /// otherwise. An order is considered reasonably priced if its limit price
    /// is lower (or "worse" for the order owner) than the `Pricegraph`
    /// estimated limit price for the order's token pair.
    fn is_reasonably_priced_order(&self) -> bool {
        match self.estimated_limit_price {
            Some(estimated_limit_price) => self.limit_price <= estimated_limit_price,
            None => false,
        }
    }

    /// The ratio at which the order is filled - `0` being not filled at all and
    /// `1` if the order is fully filled.
    ///
    /// Returns `None` if the order was not traded.
    fn fill_ratio(&self) -> Option<f64> {
        self.trade
            .as_ref()
            .map(|trade| trade.executed_sell_amount as f64 / self.effective_sell_amount)
    }
}

fn find_trade(settlement: &Settlement, order: &Element) -> Option<Trade> {
    settlement
        .trades
        .iter()
        .filter(|trade| (trade.owner, trade.order_id) == (order.user, order.id))
        .fold(None, |acc, trade| match acc {
            Some(acc) => Some(Trade {
                executed_buy_amount: acc.executed_buy_amount + trade.executed_buy_amount,
                executed_sell_amount: acc.executed_sell_amount + trade.executed_sell_amount,
                ..acc
            }),
            None => Some(trade.clone()),
        })
}

fn find_settled_exchange_rate(settlement: &Settlement, pair: TokenPair) -> Option<f64> {
    let buy_token_index = settlement
        .solution
        .token_ids_for_price
        .iter()
        .position(|&id| id == pair.buy)?;
    let sell_token_index = settlement
        .solution
        .token_ids_for_price
        .iter()
        .position(|&id| id == pair.sell)?;

    // NOTE: Because of unit coherence, the echange rate is computed as the sell
    // price over the buy price. Prices `p` are denoted in OWL per token, and
    // exchange rates are expressed as a buy amount over a sell amount. Solving
    // for two tokens `buy` and `sell`:
    // ```
    // p(sell) / p(buy) = (OWL / t_sell) / (OWL / t_buy)
    //                  = t_buy / t_sell
    //                  = xrate(buy, sell)
    // ```
    let xrate = settlement.solution.prices[sell_token_index] as f64
        / settlement.solution.prices[buy_token_index] as f64;

    // NOTE: The executed sell amount is computed from this xrate with added
    // fees, so make sure to accout for that to compute the "real" xrate that
    // **must** respect the limit prices.
    Some(xrate / FEE_FACTOR)
}

enum TradeResult {
    /// Order was reasonably priced and fully matched by the solver.
    FullyMatched,
    /// Order was unreasonably priced and not matched at all by the solver.
    UnreasonableOrderNotMatched,

    /// The order's limit price overlaps with the solutions price vector. This
    /// indicates that the order's limit price and estimated limit price were
    /// "good", but the order was not used by the solver in its solution.
    SkippedMatchableOrder,

    /// The order's price estimate was overly pessimistic. It was considered an
    /// unreasonably priced order but still partially or fully matched by the
    /// solver.
    OverlyPessimistic,

    /// The order was reasonably priced but only partially matched by the
    /// solver.
    PartiallyMatched,
    /// The order was reasonably priced, but the solver's solution produced a
    /// price vector such that the order's limit price was not overlapping. This
    /// indicates that the `pricegraph` price estimate was wrong.
    OverlyOptimistic,
    /// No solution was submitted despite existing overlapping orders. This
    /// could indicate that additional solver constraints are not properly
    /// being accounted for.
    NoSolution,
    /// The token pair for the order was not traded. This has similar
    /// implications to the `NoSolution` variant.
    NotTraded,
}

struct Row {
    batch: BatchId,
    solved: bool,
    order_uid: String,
    meta: OrderMetadata,
    result: TradeResult,
}

struct Report<T> {
    output: T,
    total: usize,
    success: usize,
    skipped: usize,
    missed: usize,
    failed: usize,
}

impl<T> Report<T>
where
    T: Write,
{
    fn new(output: T) -> Self {
        Report {
            output,
            total: 0,
            success: 0,
            skipped: 0,
            missed: 0,
            failed: 0,
        }
    }

    fn header(&mut self) -> Result<()> {
        let rows = [
            "batch",
            "solved",
            "order",
            "sellAmount",
            "limitPrice",
            "estimatedXrate",
            "settledXrate",
            "fillAmount",
        ];
        writeln!(&mut self.output, "{}", rows.join(","))?;
        Ok(())
    }
}

impl<T> Reporting for Report<T>
where
    T: Write,
{
    type Sample = Row;
    type Summary = ();

    fn record_sample(&mut self, row: Row) -> Result<()> {
        writeln!(
            &mut self.output,
            "{},{},{},{},{},{},{},{}",
            row.batch,
            row.solved,
            row.order_uid,
            row.meta.effective_sell_amount,
            row.meta.limit_price,
            row.meta.estimated_limit_price.unwrap_or_default(),
            row.meta.settled_xrate.unwrap_or_default(),
            row.meta.fill_ratio().unwrap_or_default(),
        )?;

        self.total += 1;
        match row.result {
            TradeResult::FullyMatched | TradeResult::UnreasonableOrderNotMatched => {
                self.success += 1
            }
            TradeResult::SkippedMatchableOrder => self.skipped += 1,
            TradeResult::OverlyPessimistic => self.missed += 1,
            TradeResult::PartiallyMatched
            | TradeResult::OverlyOptimistic
            | TradeResult::NoSolution
            | TradeResult::NotTraded => self.failed += 1,
        };

        Ok(())
    }

    fn finalize(self) -> Result<()> {
        let percent = |value: usize| 100.0 * value as f64 / self.total as f64;
        println!(
            "Processed {} orders: \
             {success:.2}% correct, \
             {missed:.2}% missed, \
             {failed:.2}% failed, \
             {skipped:.2}% skipped.",
            self.total,
            success = percent(self.success),
            missed = percent(self.missed),
            failed = percent(self.failed),
            skipped = percent(self.skipped),
        );

        Ok(())
    }
}
