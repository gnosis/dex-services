use pricegraph::{Pricegraph, TokenPair, TransitiveOrder};

/// An overlapping order where buy / sell == price and the amounts take into account that the solver
/// will subtract the rounding buffer from the sell amount.
pub fn order_at_price_with_rounding_buffer(
    token_pair: TokenPair,
    limit_price: f64,
    pricegraph: &Pricegraph,
    rounding_buffer: f64,
) -> Option<TransitiveOrder> {
    let order = pricegraph.order_for_limit_price(token_pair, limit_price)?;
    // We know that an order is still overlapping if it has a limit price <= this one. The limit
    // price of this order is usually larger (better for the seller) than the user requested limit
    // price.
    let order_limit_price = order.buy / order.sell;

    // The order that the user places uses the requested limit price exactly. It is less restrictive
    // (still overlapping) than the pricegraph order because the buy amount is lower.
    let order_that_user_places = TransitiveOrder {
        sell: order.sell,
        buy: order.sell * limit_price,
    };

    // The solver sees the user order with a slightly lower sell amount.
    let limit_price_that_solver_sees =
        order_that_user_places.buy / (order_that_user_places.sell - rounding_buffer);
    if limit_price_that_solver_sees <= order_limit_price {
        // If that price is not more restrictive than the pricegraph order's limit price then it is
        // still overlapping and we can use it. This case it much more likely than the other branch
        // because you would have to hit a narrow price so that there is no room to adjust for the
        // small (compared to buy and sell amounts) rounding buffer.
        Some(order_that_user_places)
    } else {
        // Otherwise we cannot keep the limit_price exactly the same as requested. Since rounding
        // buffers are small compared to traded amounts the introduced error is small.
        Some(TransitiveOrder {
            sell: order_that_user_places.sell + rounding_buffer,
            buy: order_that_user_places.buy,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pricegraph::{Element, PriceFraction, TokenPair, UserId, Validity, FEE_FACTOR};

    #[test]
    fn order_at_price_likely_branch() {
        let denominator = 100_000;
        let elements = vec![Element {
            user: UserId::zero(),
            balance: 100_000.into(),
            pair: TokenPair { buy: 0, sell: 1 },
            valid: Validity { from: 0, to: 0 },
            price: PriceFraction {
                numerator: 100_000,
                denominator,
            },
            remaining_sell_amount: 100_000,
            id: 0,
        }];
        let pricegraph = Pricegraph::new(elements);

        let limit_price = 0.5;
        let rounding_buffer = 1000.0;
        let result = order_at_price_with_rounding_buffer(
            TokenPair { buy: 1, sell: 0 },
            limit_price,
            &pricegraph,
            rounding_buffer,
        );
        assert_eq!(
            result,
            Some(TransitiveOrder {
                buy: (denominator as f64) * limit_price * FEE_FACTOR,
                sell: (denominator as f64) * FEE_FACTOR,
            })
        )
    }

    #[test]
    fn order_at_price_unlikely_branch() {
        let denominator = 100_000;
        let elements = vec![Element {
            user: UserId::zero(),
            balance: 100_000.into(),
            pair: TokenPair { buy: 0, sell: 1 },
            valid: Validity { from: 0, to: 0 },
            price: PriceFraction {
                numerator: 100_000,
                denominator,
            },
            remaining_sell_amount: 100_000,
            id: 0,
        }];
        let pricegraph = Pricegraph::new(elements);

        let limit_price = 0.998;
        let rounding_buffer = 1000.0;
        let result = order_at_price_with_rounding_buffer(
            TokenPair { buy: 1, sell: 0 },
            limit_price,
            &pricegraph,
            rounding_buffer,
        );
        let sell = (denominator as f64) * FEE_FACTOR;
        assert_eq!(
            result,
            Some(TransitiveOrder {
                buy: sell * limit_price,
                sell: sell + rounding_buffer,
            })
        )
    }
}
