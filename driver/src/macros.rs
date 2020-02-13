//! Module containing utility macros for sharing for in the crate.

/// Macro for instanciating a `HashMap`. Note that `ToOwned::to_owned` is called
/// for keys, so things like `str` keys atomatically get turned into `String`s.
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
