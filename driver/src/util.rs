use ethcontract::U256;
use std::future::Future;

pub trait CeiledDiv {
    /// Panics on overflow.
    fn ceiled_div(&self, divisor: Self) -> Self;
}

impl CeiledDiv for u128 {
    fn ceiled_div(&self, divisor: u128) -> u128 {
        // ceil(p / float(q)) == (p + q - 1) / q
        self.checked_add(divisor)
            .unwrap()
            .checked_sub(1)
            .unwrap()
            .checked_div(divisor)
            .unwrap()
    }
}

impl CeiledDiv for U256 {
    fn ceiled_div(&self, divisor: U256) -> U256 {
        //ceil(p / float(q)) == (p + q - 1) / q
        self.checked_add(divisor)
            .unwrap()
            .checked_sub(1.into())
            .unwrap()
            .checked_div(divisor)
            .unwrap()
    }
}

pub trait CheckedConvertU128 {
    fn as_u128_checked(&self) -> Option<u128>;
}

impl CheckedConvertU128 for U256 {
    fn as_u128_checked(&self) -> Option<u128> {
        if *self <= U256::from(u128::max_value()) {
            Some(self.low_u128())
        } else {
            None
        }
    }
}

pub trait FutureWaitExt: Future {
    fn wait(self) -> Self::Output;
}

impl<F> FutureWaitExt for F
where
    F: Future,
{
    fn wait(self) -> Self::Output {
        futures::executor::block_on(self)
    }
}

#[cfg(test)]
pub mod test_util {
    use std::collections::HashMap;
    use std::hash::Hash;

    pub fn map_from_slice<T: Copy + Eq + Hash, U: Copy>(arr: &[(T, U)]) -> HashMap<T, U> {
        arr.iter().copied().collect()
    }
}
