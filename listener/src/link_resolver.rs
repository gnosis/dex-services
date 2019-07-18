use graph::components::link_resolver::JsonValueStream;
use graph::data::subgraph::Link;
use graph::prelude::LinkResolver as LinkResolverTrait;

use futures::prelude::*;
use slog::Logger;

pub struct LocalLinkResolver {}

impl LinkResolverTrait for LocalLinkResolver {
    /// Fetches the link contents as bytes.
    fn cat(
        &self,
        _logger: &Logger,
        _link: &Link,
    ) -> Box<Future<Item = Vec<u8>, Error = failure::Error> + Send> {
        unimplemented!();
    }

    fn json_stream(
        &self,
        _link: &Link,
    ) -> Box<Future<Item = JsonValueStream, Error = failure::Error> + Send + 'static> {
        unimplemented!();
    }
}