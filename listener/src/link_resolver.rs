use graph::components::link_resolver::JsonValueStream;
use graph::data::subgraph::Link;
use graph::prelude::LinkResolver as LinkResolverTrait;

use futures::prelude::*;
use futures::future::*;
use slog::Logger;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;

fn read_file(file: &str) -> Result<Vec<u8>, failure::Error> {
    let path = format!("subgraph_definition/{}", 
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

pub struct LocalLinkResolver {}

impl LinkResolverTrait for LocalLinkResolver {
    /// Fetches the link contents as bytes.
    fn cat(
        &self,
        logger: &Logger,
        link: &Link,
    ) -> Box<Future<Item = Vec<u8>, Error = failure::Error> + Send> {
        info!(logger, "Resolving link {}", &link.link);
        match read_file(&link.link) {
            Ok(res) => Box::new(ok(res)),
            Err(e) => Box::new(err(e))
        }
    }

    fn json_stream(
        &self,
        _link: &Link,
    ) -> Box<Future<Item = JsonValueStream, Error = failure::Error> + Send + 'static> {
        unimplemented!();
    }
}