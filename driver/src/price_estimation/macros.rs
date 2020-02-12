//! Module implements some useful macros for price estimation.

/// Utility macro to facilitate the generation of the `TOKEN_PROXIES` map. In
/// particular it generates sets for equivalent tokens.
///
/// This might be an abuse of macros...
macro_rules! token_proxies {
    (const $n:ident = { $( $( $( $token:ident ),* => $( $proxy:ident ),*)? $(<=> $( $equiv:ident ),* )? ;)* }) => {
        lazy_static! {
            static ref $n: HashMap<String, HashSet<String>> = {
                let mut token_proxies = HashMap::new();
                $(
                    // Generate forward mapping of token to list of proxies.
                    $(
                        let tokens = vec![$(stringify!($token)),*];
                        for token in tokens {
                            token_proxies.insert(token.into(), {
                                let mut proxies = HashSet::new();
                                $(
                                    proxies.insert(stringify!($proxy).into());
                                )*
                                proxies
                            });
                        }
                    )*

                    // Generate mapping between equivalent tokens, each token
                    // will have its own entry in the token proxy table.
                    $(
                        {
                            let mut equivalent_tokens = HashSet::new();
                            $(
                                equivalent_tokens.insert(stringify!($equiv).to_owned());
                            )*
                            for token in equivalent_tokens.iter() {
                                let mut proxies = equivalent_tokens.clone();
                                proxies.remove(token);
                                token_proxies.insert(token.into(), proxies);
                            }
                        }
                    )*
                )*
                token_proxies
            };
        }
    };
}
