use std::collections::HashMap;
use std::hash::Hash;

pub fn map_from_slice<T: Copy + Eq + Hash, U: Copy>(arr: &[(T, U)]) -> HashMap<T, U> {
    arr.iter().copied().collect()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_map_from_slice() {
        let mut expected = HashMap::new();
        expected.insert(0u16, 1u128);
        expected.insert(1u16, 2u128);
        assert_eq!(map_from_slice(&[(0, 1), (1, 2)]), expected);
    }
}
