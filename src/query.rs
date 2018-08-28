use finchers::endpoint::{Context, Endpoint, EndpointResult};
use finchers::endpoints::{body, method};
use finchers::error;
use finchers::error::{Error, Never};
use finchers::output::payload::Once;
use finchers::output::{Output, OutputContext};

use std::mem::PinMut;
use std::task;
use std::task::Poll;

use futures::future::{Future, TryFuture};
use futures::try_ready;
use pin_utils::unsafe_pinned;

use juniper::http::GraphQLRequest;
use juniper::{GraphQLType, RootNode};

use http::{header, Response, StatusCode};
use tokio::prelude::Async as Async01;
use tokio_threadpool::blocking;

use crate::maybe_done::MaybeDone;

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
/// let acquire_ctx = ...;
///
/// let query_endpoint = route!(/ "query")
///     .and(finchers_juniper::query(schema, acquire_ctx));
/// ```
pub fn query<QueryT, MutationT, CtxT, E>(
    root_node: RootNode<'static, QueryT, MutationT>,
    context_endpoint: E,
) -> Query<QueryT, MutationT, CtxT, E>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
    for<'a> E: Endpoint<'a, Output = (CtxT,)>,
{
    Query {
        root_node,
        context_endpoint,
    }
}

#[allow(missing_docs)]
pub struct Query<QueryT, MutationT, CtxT, E>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    root_node: RootNode<'static, QueryT, MutationT>,
    context_endpoint: E,
}

impl<'a, QueryT, MutationT, CtxT, E> Endpoint<'a> for Query<QueryT, MutationT, CtxT, E>
where
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
    E: Endpoint<'a, Output = (CtxT,)>,
{
    type Output = (QueryResponse,);
    type Future = QueryFuture<'a, QueryT, MutationT, CtxT, E::Future>;

    fn apply(&'a self, cx: &mut Context) -> EndpointResult<Self::Future> {
        let request = method::post(body::json()).apply(cx)?;
        let context = self.context_endpoint.apply(cx)?;
        Ok(QueryFuture {
            root_node: &self.root_node,
            request: MaybeDone::new(request),
            context: MaybeDone::new(context),
        })
    }
}

pub struct QueryFuture<'a, QueryT, MutationT, CtxT, R>
where
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    R: TryFuture<Ok = (CtxT,), Error = Error> + 'a,
{
    root_node: &'a RootNode<'static, QueryT, MutationT>,
    request: MaybeDone<<body::Json<GraphQLRequest> as Endpoint<'a>>::Future>,
    context: MaybeDone<R>,
}

impl<'a, QueryT, MutationT, CtxT, R> QueryFuture<'a, QueryT, MutationT, CtxT, R>
where
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    R: TryFuture<Ok = (CtxT,), Error = Error> + 'a,
{
    unsafe_pinned!(request: MaybeDone<<body::Json<GraphQLRequest> as Endpoint<'a>>::Future>);
    unsafe_pinned!(context: MaybeDone<R>);

    fn execute(mut self: PinMut<Self>) -> Result<QueryResponse, serde_json::Error> {
        let (request,) = self.request().take_ok().unwrap();
        let (context,) = self.context().take_ok().unwrap();
        let response = request.execute(self.root_node, &context);
        Ok(QueryResponse {
            is_ok: response.is_ok(),
            body: serde_json::to_vec(&response)?,
        })
    }
}

impl<'a, QueryT, MutationT, CtxT, R> Future for QueryFuture<'a, QueryT, MutationT, CtxT, R>
where
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    R: TryFuture<Ok = (CtxT,), Error = Error> + 'a,
{
    type Output = Result<(QueryResponse,), Error>;

    fn poll(mut self: PinMut<Self>, cx: &mut task::Context) -> Poll<Self::Output> {
        try_ready!(self.request().poll_ready(cx));
        try_ready!(self.context().poll_ready(cx));
        match blocking(move || self.execute()) {
            Ok(Async01::Ready(Ok(response))) => Poll::Ready(Ok((response,))),
            Ok(Async01::Ready(Err(err))) => Poll::Ready(Err(error::fail(err))),
            Ok(Async01::NotReady) => Poll::Pending,
            Err(err) => Poll::Ready(Err(error::fail(err))),
        }
    }
}

#[derive(Debug)]
pub struct QueryResponse {
    is_ok: bool,
    body: Vec<u8>,
}

impl Output for QueryResponse {
    type Body = Once<Vec<u8>>;
    type Error = Never;

    fn respond(self, _: &mut OutputContext) -> Result<Response<Self::Body>, Self::Error> {
        let status = if self.is_ok {
            StatusCode::OK
        } else {
            StatusCode::BAD_REQUEST
        };
        Ok(Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Once::new(self.body))
            .expect("valid response"))
    }
}
