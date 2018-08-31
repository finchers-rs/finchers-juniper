#![feature(rust_2018_preview)]
#![feature(pin, futures_api, arbitrary_self_types)]

//! Endpoints for Juniper integration.

extern crate bytes;
extern crate failure;
extern crate finchers;
extern crate futures;
extern crate http;
extern crate juniper;
extern crate percent_encoding;
extern crate pin_utils;
extern crate serde;
extern crate tokio;
extern crate tokio_threadpool;

mod execute;
mod graphiql;
mod maybe_done;
mod request;

pub use crate::execute::Execute;
pub use crate::graphiql::{graphiql, GraphiQL};
pub use crate::request::{request, GraphQLRequest, GraphQLResponse, RequestEndpoint};

use finchers::endpoint::Endpoint;
use juniper::{GraphQLType, RootNode};

pub trait EndpointGraphQLExt<'a>: Endpoint<'a> + Sized {
    /// Creates an endpoint which handles a GraphQL request.
    ///
    /// The endpoint contains a `RootNode` associated with a type `CtxT`,
    /// and an endpoint which returns the value of `CtxT`.
    /// Within `context_endpoint`, the authentication process and establishing
    /// the DB connection (and so on) can be executed.
    ///
    /// The future returned by this endpoint will wait for the reception of
    /// the GraphQL request and the preparation of `CtxT` to be completed,
    /// and then transition to the blocking state using tokio's blocking API
    /// and executes the GraphQL request on the current thread.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let fetch_context = ...;
    /// let schema = ...;
    ///
    /// let query_endpoint = route!(/ "query")
    ///     .and(fetch_context)
    ///     .handle_graphql(schema);
    /// ```
    fn handle_graphql<QueryT, MutationT, CtxT>(
        self,
        root_node: RootNode<'static, QueryT, MutationT>,
    ) -> Execute<Self, QueryT, MutationT, CtxT>
    where
        Self: Endpoint<'a, Output = (CtxT,)>,
        QueryT: GraphQLType<Context = CtxT>,
        MutationT: GraphQLType<Context = CtxT>,
    {
        Execute {
            context_endpoint: self,
            request_endpoint: crate::request::request(),
            root_node,
        }
    }
}

impl<'a, E> EndpointGraphQLExt<'a> for E where E: Endpoint<'a> {}
