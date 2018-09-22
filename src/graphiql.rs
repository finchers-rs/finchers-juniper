use finchers::endpoint::{Context, Endpoint, EndpointResult};
use finchers::error::{Error, Never};
use finchers::output::payload::Once;
use finchers::output::{Output, OutputContext};

use futures::{Future, Poll};

use bytes::Bytes;
use http::{header, Response};
use juniper::http::graphiql::graphiql_source;

/// Creates an endpoint which returns a generated GraphiQL interface.
pub fn graphiql(endpoint_url: impl AsRef<str>) -> GraphiQL {
    GraphiQL {
        source: graphiql_source(endpoint_url.as_ref()).into(),
    }
}

#[allow(missing_docs)]
#[derive(Debug)]
pub struct GraphiQL {
    source: Bytes,
}

impl GraphiQL {
    /// Regenerate the GraphiQL interface with the specified endpoint URL.
    pub fn regenerate(&mut self, endpoint_url: impl AsRef<str>) {
        self.source = graphiql_source(endpoint_url.as_ref()).into();
    }
}

impl<'a> Endpoint<'a> for GraphiQL {
    type Output = (GraphiQLSource,);
    type Future = GraphiQLFuture<'a>;

    fn apply(&'a self, _: &mut Context<'_>) -> EndpointResult<Self::Future> {
        Ok(GraphiQLFuture(&self.source))
    }
}

#[derive(Debug)]
pub struct GraphiQLFuture<'a>(&'a Bytes);

impl<'a> Future for GraphiQLFuture<'a> {
    type Item = (GraphiQLSource,);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok((GraphiQLSource(self.0.clone()),).into())
    }
}

#[derive(Debug)]
pub struct GraphiQLSource(Bytes);

impl Output for GraphiQLSource {
    type Body = Once<Bytes>;
    type Error = Never;

    fn respond(self, _: &mut OutputContext<'_>) -> Result<Response<Self::Body>, Self::Error> {
        Ok(Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Once::new(self.0))
            .expect("valid response"))
    }
}
