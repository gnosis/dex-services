mod encoding;

use crate::encoding::{Element, InvalidLength};
use std::collections::HashSet;

pub type TokenId = u16;
pub type UserId = [u8; 20];

pub struct Orderbook {
    _tokens: HashSet<TokenId>,
}

impl Orderbook {
    pub fn read(orders: impl AsRef<[u8]>) -> Result<Orderbook, InvalidLength> {
        let mut tokens = HashSet::new();
        for element in Element::read_all(orders.as_ref())? {
            tokens.insert(element.pair.0);
            tokens.insert(element.pair.1);
        }

        Ok(Orderbook { _tokens: tokens })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn assert() {
        assert_eq!(1, 1);
    }
}
