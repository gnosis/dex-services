use graph::components::link_resolver::JsonValueStream;
use graph::data::subgraph::Link;
use graph::prelude::LinkResolver as LinkResolverTrait;

use futures::future::*;
use slog::{info, Logger};
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use std::time::Duration;

fn read_file(file: &str) -> Result<Vec<u8>, failure::Error> {
    let path = format!(
        "listener/subgraph_definition/{}",
        Path::new(file)
            .iter()
            .last()
            .and_then(|p| p.to_str())
            .ok_or_else(|| failure::err_msg("invalid file name"))?
    );
    let mut f = File::open(&path)?;
    let mut buffer = Vec::new();
    f.read_to_end(&mut buffer)?;
    Ok(buffer)
}

#[derive(Clone)]
pub struct LocalLinkResolver;

impl LinkResolverTrait for LocalLinkResolver {
    fn with_timeout(self, _timeout: Duration) -> Self {
        self
    }

    fn with_retries(self) -> Self {
        self
    }

    fn cat(
        &self,
        logger: &Logger,
        link: &Link,
    ) -> Box<dyn Future<Item = Vec<u8>, Error = failure::Error> + Send> {
        info!(logger, "Resolving link {}", &link.link);
        match read_file(&link.link) {
            Ok(res) => Box::new(ok(res)),
            Err(e) => Box::new(err(e)),
        }
    }

    fn json_stream(
        &self,
        _link: &Link,
    ) -> Box<dyn Future<Item = JsonValueStream, Error = failure::Error> + Send + 'static> {
        unimplemented!();
    }
}
