#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use pricegraph::{Element, Orderbook, Price, TokenPair, Validity, H160, U256};

// Fuzz creation and usage of Orderbook.

fuzz_target!(|elements: Vec<ArbitraryElement>| {
    let _ = Orderbook::from_elements(elements.into_iter().map(|element| element.into()));
});

/// Remote `Arbitrary` implementation for `Element` as Cargo lock files (and
/// therefore workspaces) only allow one set of features per dependency.
#[derive(Arbitrary, Debug)]
struct ArbitraryElement {
    user: [u8; 20],
    balance: [u64; 4],
    pair: (u16, u16),
    valid: (u32, u32),
    price: (u128, u128),
    remaining_sell_amount: u128,
    id: u16,
}

impl Into<Element> for ArbitraryElement {
    fn into(self) -> Element {
        Element {
            user: H160(self.user),
            balance: U256(self.balance),
            pair: TokenPair {
                buy: self.pair.0,
                sell: self.pair.1,
            },
            valid: Validity {
                from: self.valid.0,
                to: self.valid.1,
            },
            price: Price {
                numerator: self.price.0,
                denominator: self.price.1,
            },
            remaining_sell_amount: self.remaining_sell_amount,
            id: self.id,
        }
    }
}
