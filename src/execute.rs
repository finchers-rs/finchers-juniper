use finchers::endpoint::{Context, Endpoint, EndpointResult};
use finchers::error;
use finchers::error::Error;

use std::mem::PinMut;
use std::task;
use std::task::Poll;

use futures::future::{Future, TryFuture};
use futures::try_ready;
use pin_utils::unsafe_pinned;

use juniper::{GraphQLType, RootNode};

use tokio::prelude::Async as Async01;
use tokio_threadpool::blocking;

use crate::maybe_done::MaybeDone;
use crate::request::{request, GraphQLResponse, RequestEndpoint, RequestFuture};

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
/// let schema = ...;
///
/// let query_endpoint = route!(/ "query")
///     .and(finchers_juniper::execute(acquire_ctx, schema));
/// ```
pub fn execute<E, QueryT, MutationT, CtxT>(
    context_endpoint: E,
    root_node: RootNode<'static, QueryT, MutationT>,
) -> Execute<E, QueryT, MutationT, CtxT>
where
    for<'a> E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    Execute {
        root_node,
        context_endpoint,
        request_endpoint: request(),
    }
}

#[allow(missing_docs)]
pub struct Execute<E, QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    root_node: RootNode<'static, QueryT, MutationT>,
    context_endpoint: E,
    request_endpoint: RequestEndpoint,
}

impl<'a, E, QueryT, MutationT, CtxT> Endpoint<'a> for Execute<E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    CtxT: 'a,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
{
    type Output = (GraphQLResponse,);
    type Future = ExecuteFuture<'a, E::Future, QueryT, MutationT, CtxT>;

    fn apply(&'a self, cx: &mut Context) -> EndpointResult<Self::Future> {
        let request = self.request_endpoint.apply(cx)?;
        let context = self.context_endpoint.apply(cx)?;
        Ok(ExecuteFuture {
            request: MaybeDone::new(request),
            context: MaybeDone::new(context),
            root_node: &self.root_node,
        })
    }
}

pub struct ExecuteFuture<'a, F, QueryT, MutationT, CtxT>
where
    F: TryFuture<Ok = (CtxT,), Error = Error> + 'a,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
{
    context: MaybeDone<F>,
    request: MaybeDone<RequestFuture<'a>>,
    root_node: &'a RootNode<'static, QueryT, MutationT>,
}

impl<'a, F, QueryT, MutationT, CtxT> ExecuteFuture<'a, F, QueryT, MutationT, CtxT>
where
    F: TryFuture<Ok = (CtxT,), Error = Error> + 'a,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
{
    unsafe_pinned!(context: MaybeDone<F>);
    unsafe_pinned!(request: MaybeDone<RequestFuture<'a>>);
}

impl<'a, F, QueryT, MutationT, CtxT> Future for ExecuteFuture<'a, F, QueryT, MutationT, CtxT>
where
    F: TryFuture<Ok = (CtxT,), Error = Error> + 'a,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
{
    type Output = Result<(GraphQLResponse,), Error>;

    fn poll(mut self: PinMut<Self>, cx: &mut task::Context) -> Poll<Self::Output> {
        try_ready!(self.request().poll_ready(cx));
        try_ready!(self.context().poll_ready(cx));
        match blocking(move || {
            let (request,) = self.request().take_ok().unwrap();
            let (context,) = self.context().take_ok().unwrap();
            request.execute(self.root_node, &context)
        }) {
            Ok(Async01::Ready(response)) => Poll::Ready(Ok((response,))),
            Ok(Async01::NotReady) => Poll::Pending,
            Err(err) => Poll::Ready(Err(error::fail(err))),
        }
    }
}
