//! Module containing utility macros for sharing for in the crate.

#[cfg_attr(test, macro_use)]
#[cfg(test)]
mod test_macros {
    macro_rules! hash_map {
        ($( $key:expr => $value:expr ),* $(,)?) => {{
            #[allow(unused_mut)]
            let mut map = std::collections::HashMap::new();
            $(
                map.insert(($key).to_owned(), $value);
            )*
            map
        }}
    }
}
