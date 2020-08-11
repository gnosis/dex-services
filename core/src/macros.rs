//! Module containing utility macros for sharing in the crate.

/// Macro for instantiating a `HashMap`.
macro_rules! hash_map {
    ($($tt:tt)*) => {
        std_map!(<HashMap> $($tt)*)
    }
}

/// Macro for instantiating a `BTreeMap`.
#[cfg(test)]
macro_rules! btree_map {
    ($($tt:tt)*) => {
        std_map!(<BTreeMap> $($tt)*)
    }
}

/// Implementation macro for instantiating a standard library map type like
/// `HashMap` or `BTreeMap`. Note that `ToOwned::to_owned` is called for keys,
/// so things like `str` keys automatically get turned into `String`s.
macro_rules! std_map {
    (<$t:ident> $( $key:expr => $value:expr ),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut map = std::collections::$t::new();
        $(
            map.insert(($key).to_owned(), $value);
        )*
        map
    }}
}

macro_rules! immediate {
    ($expression:expr) => {
        futures::future::FutureExt::boxed(async move { $expression })
    };
}

#[cfg(test)]
macro_rules! nonzero {
    ($expression:expr) => {
        std::num::NonZeroU128::new($expression).unwrap()
    };
}
