use ethcontract::U256;
use futures::future::{BoxFuture, FutureExt as _};
use std::{
    future::Future,
    time::{Duration, Instant, SystemTime},
};

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

#[cfg_attr(test, mockall::automock)]
pub trait AsyncSleeping: 'static + Send + Sync {
    #[must_use]
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
        async_std::task::sleep(duration).boxed()
    }
}

pub struct AsyncSleep;
impl AsyncSleeping for AsyncSleep {}

#[cfg_attr(test, mockall::automock)]
pub trait Now: 'static + Send + Sync {
    fn system_now(&self) -> SystemTime;
    fn instant_now(&self) -> Instant;
}

pub fn default_now() -> impl Now {
    DefaultNow {}
}

struct DefaultNow;
impl Now for DefaultNow {
    fn system_now(&self) -> SystemTime {
        SystemTime::now()
    }
    fn instant_now(&self) -> Instant {
        Instant::now()
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

#[cfg(test)]
pub mod tests {
    use super::test_util::*;
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_checked_u256_to_u128() {
        assert_eq!(Some(42u128), U256::from(42).as_u128_checked());
        assert_eq!(
            Some(u128::max_value()),
            U256::from(u128::max_value()).as_u128_checked(),
        );
        assert_eq!(
            None,
            (U256::from(u128::max_value()) + U256::one()).as_u128_checked(),
        );
        assert_eq!(None, U256::max_value().as_u128_checked(),);
    }

    #[test]
    fn test_ceiled_div_u128() {
        assert_eq!(0u128.ceiled_div(10), 0);
        assert_eq!(1u128.ceiled_div(10), 1);
        assert_eq!(10u128.ceiled_div(10), 1);
    }

    #[test]
    #[should_panic]
    fn test_ceiled_div_by_0_u128() {
        1u128.ceiled_div(0);
    }

    #[test]
    #[should_panic]
    fn test_ceiled_div_overflow_u128() {
        u128::max_value().ceiled_div(1);
    }

    #[test]
    fn test_ceiled_div_u256() {
        assert_eq!(U256::from(0).ceiled_div(U256::from(10)), U256::from(0));
        assert_eq!(U256::from(1).ceiled_div(U256::from(10)), U256::from(1));
        assert_eq!(U256::from(10).ceiled_div(U256::from(10)), U256::from(1));
    }

    #[test]
    #[should_panic]
    fn test_ceiled_div_by_0_u256() {
        U256::one().ceiled_div(U256::zero());
    }

    #[test]
    #[should_panic]
    fn test_ceiled_div_overflow_u256() {
        U256::max_value().ceiled_div(U256::from(1));
    }

    #[test]
    fn test_map_from_slice() {
        let mut expected = HashMap::new();
        expected.insert(0u16, 1u128);
        expected.insert(1u16, 2u128);
        assert_eq!(map_from_slice(&[(0, 1), (1, 2)]), expected);
    }
}
