#![no_main]
use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use pricegraph::{Element, Pricegraph, TokenId, TokenPair};
use std::iter::once;

// Limit the maximum token id that is allowed to appear in the generated orders. Without this we can
// make orderbook creation too slow if one order contains a large buy or sell token id.
const MAX_TOKEN_ID: u16 = 16;

// Fuzz creation and usage of Orderbook.

#[derive(Arbitrary, Debug)]
enum Operation {
    OrderForSellAmount {
        pair: TokenPair,
        sell_amount: f64,
    },
    TransitiveOrderbook {
        base: TokenId,
        quote: TokenId,
        spread: Option<f64>,
    },
}

#[derive(Arbitrary, Debug)]
struct Arguments {
    elements: Vec<Element>,
    operation: Operation,
}

fn largest_token_id(elements: &[Element]) -> Option<TokenId> {
    elements
        .iter()
        .flat_map(|e| once(e.pair.buy).chain(once(e.pair.sell)))
        .max()
}

fuzz_target!(|arguments: Arguments| {
    if largest_token_id(&arguments.elements).unwrap_or(0) > MAX_TOKEN_ID {
        return;
    }

    let pricegraph = Pricegraph::new(arguments.elements);
    match arguments.operation {
        Operation::OrderForSellAmount { pair, sell_amount } => {
            pricegraph.order_for_sell_amount(pair, sell_amount);
        }
        Operation::TransitiveOrderbook {
            base,
            quote,
            spread,
        } => {
            if let Some(spread) = spread {
                if !spread.is_finite() || spread <= 0.0 {
                    return;
                }
            }
            pricegraph.transitive_orderbook(base, quote, spread);
        }
    };
});
