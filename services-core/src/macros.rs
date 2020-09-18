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
    ($expression:expr) => {{
        let value = $expression;
        futures::future::FutureExt::boxed(async move { value })
    }};
}

/// Macro for generating an enum to be used as an argument, with `FromStr`
/// implementation as well a utility method for iterating variants.
macro_rules! arg_enum {
    (
        $(#[$attr:meta])*
        $vis:vis enum $name:ident {$(
            $(#[$variant_attr:meta])*
            $variant:ident,
        )*}
    ) => {
        $(#[$attr])*
        $vis enum $name {$(
            $(#[$variant_attr])*
            $variant,
        )*}

        impl $name {
            /// Returns a slice with all variants for this enum.
            pub fn variants() -> &'static [Self] {
                &[$(
                    Self::$variant,
                )*]
            }

            /// Returns a slice with all variant names for this enum.
            pub fn variant_names() -> &'static [&'static str] {
                &[$(
                    stringify!($variant),
                )*]
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str(match self {$(
                    Self::$variant => stringify!($variant),
                )*})
            }
        }

        impl std::str::FromStr for $name {
            type Err = anyhow::Error;

            fn from_str(value: &str) -> anyhow::Result<Self> {
                match value {
                    $(
                        _ if value.eq_ignore_ascii_case(stringify!($variant)) => {
                            Ok(Self::$variant)
                        }
                    )*
                    _ => anyhow::bail!(
                        "unknown {} variant '{}'",
                        stringify!(name),
                        value,
                    ),
                }
            }
        }
    };
}

#[cfg(test)]
macro_rules! nonzero {
    ($expression:expr) => {
        std::num::NonZeroU128::new($expression).unwrap()
    };
}
