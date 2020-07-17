//! Module containing test utilities and macros.

#[path = "../data/mod.rs"]
pub mod data;

use crate::encoding::UserId;

/// Returns a `UserId` for a test user index.
///
/// This method is meant to be used in conjunction with orderbooks created
/// with the `orderbook` macro.
pub fn user_id(id: u8) -> UserId {
    UserId::repeat_byte(id)
}

/// Macro for constructing an orderbook using a DSL for testing purposes.
macro_rules! orderbook {
    (
        users {$(
            @ $user:tt {$(
                token $token:tt => $balance:expr,
            )*}
        )*}
        orders {$(
            owner @ $owner:tt
            buying $buy:tt [ $buy_amount:expr ]
            selling $sell:tt [ $sell_amount:expr ] $( ($remaining:expr) )?
        ,)*}
    ) => {{
        #[allow(unused_mut, unused_variables)]
        let mut balances = std::collections::HashMap::<
            (u8, $crate::encoding::TokenId), $crate::encoding::U256,
        >::new();
        $($(
            balances.insert(($user, $token), $crate::U256::from($balance as u128));
        )*)*
        #[allow(unused_mut, unused_variables)]
        let mut users = std::collections::HashMap::<
            u8, $crate::encoding::OrderId,
        >::new();
        let elements = vec![$(
            $crate::encoding::Element {
                user: $crate::test::user_id($owner),
                balance: balances[&($owner, $sell)],
                pair: $crate::encoding::TokenPair {
                    buy: $buy,
                    sell: $sell,
                },
                valid: $crate::encoding::Validity {
                    from: 0,
                    to: u32::MAX,
                },
                price: $crate::encoding::PriceFraction {
                    numerator: $buy_amount,
                    denominator: $sell_amount,
                },
                remaining_sell_amount: match &[$sell_amount, $($remaining)?][..] {
                    [_, remaining] => *remaining,
                    _ => $sell_amount,
                },
                id: {
                    let count = users.entry($owner).or_insert(0);
                    let id = *count;
                    *count += 1;
                    id
                },
            },
        )*];
        $crate::orderbook::Orderbook::from_elements(elements)
    }};
}

/// Macro for constructing a pricegraph API instance using a DSL for testing
/// purposes.
macro_rules! pricegraph {
    ($($arg:tt)*) => {
        $crate::Pricegraph::from_orderbook(orderbook!($($arg)*))
    };
}

pub mod prelude {
    pub use super::*;
    pub use assert_approx_eq::assert_approx_eq;
}
