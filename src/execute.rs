use finchers::endpoint::wrapper::Wrapper;
use finchers::endpoint::{Context, Endpoint, EndpointResult};
use finchers::error::Error;

use futures::future::{Future, TryFuture};
use futures::ready;
use futures::task;
use futures::task::Poll;

use pin_utils::unsafe_pinned;
use std::pin::PinMut;

use juniper::{GraphQLType, RootNode};
use std::fmt;
use tokio::prelude::Async as Async01;
use tokio_threadpool::blocking;

use crate::maybe_done::MaybeDone;
use crate::request::{GraphQLResponse, RequestEndpoint};

/// Create a `Wrapper` for building a GraphQL endpoint using the specified `RootNode`.
pub fn execute<QueryT, MutationT, CtxT>(
    root_node: RootNode<'static, QueryT, MutationT>,
) -> Execute<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    Execute { root_node }
}

#[allow(missing_docs)]
pub struct Execute<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    root_node: RootNode<'static, QueryT, MutationT>,
}

impl<QueryT, MutationT, CtxT> fmt::Debug for Execute<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Execute").finish()
    }
}

impl<'a, E, QueryT, MutationT, CtxT> Wrapper<'a, E> for Execute<QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    type Output = (GraphQLResponse,);
    type Endpoint = ExecuteEndpoint<E, QueryT, MutationT, CtxT>;

    fn wrap(self, endpoint: E) -> Self::Endpoint {
        ExecuteEndpoint {
            context: endpoint,
            request: crate::request::request(),
            root_node: self.root_node,
        }
    }
}

pub struct ExecuteEndpoint<E, QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    context: E,
    request: RequestEndpoint,
    root_node: RootNode<'static, QueryT, MutationT>,
}

impl<E, QueryT, MutationT, CtxT> fmt::Debug for ExecuteEndpoint<E, QueryT, MutationT, CtxT>
where
    E: fmt::Debug,
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExecuteEndpoint")
            .field("context", &self.context)
            .field("request", &self.request)
            .finish()
    }
}

impl<'a, E, QueryT, MutationT, CtxT> Endpoint<'a> for ExecuteEndpoint<E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    type Output = (GraphQLResponse,);
    type Future = ExecuteFuture<'a, E::Future, QueryT, MutationT, CtxT>;

    fn apply(&'a self, cx: &mut Context<'_>) -> EndpointResult<Self::Future> {
        let context = self.context.apply(cx)?;
        let request = self.request.apply(cx)?;
        Ok(ExecuteFuture {
            context: MaybeDone::new(context),
            request: MaybeDone::new(request),
            root_node: &self.root_node,
        })
    }
}

pub struct ExecuteFuture<'a, Fut, QueryT, MutationT, CtxT: 'a>
where
    Fut: TryFuture<Ok = (CtxT,), Error = Error>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    context: MaybeDone<Fut>,
    request: MaybeDone<<RequestEndpoint as Endpoint<'a>>::Future>,
    root_node: &'a RootNode<'static, QueryT, MutationT>,
}

impl<'a, Fut, QueryT, MutationT, CtxT> fmt::Debug
    for ExecuteFuture<'a, Fut, QueryT, MutationT, CtxT>
where
    Fut: TryFuture<Ok = (CtxT,), Error = Error> + fmt::Debug,
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
    CtxT: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExecuteFuture")
            .field("context", &self.context)
            .field("request", &self.request)
            .finish()
    }
}

impl<'a, Fut, QueryT, MutationT, CtxT> ExecuteFuture<'a, Fut, QueryT, MutationT, CtxT>
where
    Fut: TryFuture<Ok = (CtxT,), Error = Error>,
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
    CtxT: 'a,
{
    unsafe_pinned!(context: MaybeDone<Fut>);
    unsafe_pinned!(request: MaybeDone<<RequestEndpoint as Endpoint<'a>>::Future>);

    fn execute(mut self: PinMut<'_, Self>) -> GraphQLResponse {
        let (context,) = self
            .context()
            .take_ok()
            .expect("The context has already taken");
        let (request,) = self
            .request()
            .take_ok()
            .expect("The request has already taken");
        request.execute(self.root_node, &context)
    }
}

impl<'a, Fut, QueryT, MutationT, CtxT> Future for ExecuteFuture<'a, Fut, QueryT, MutationT, CtxT>
where
    Fut: TryFuture<Ok = (CtxT,), Error = Error>,
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
    CtxT: 'a,
{
    type Output = Result<(GraphQLResponse,), Error>;

    fn poll(mut self: PinMut<'_, Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        ready!(self.context().poll_ready(cx)?);
        ready!(self.request().poll_ready(cx)?);
        match blocking(move || self.execute()) {
            Ok(Async01::Ready(response)) => Poll::Ready(Ok((response,))),
            Ok(Async01::NotReady) => Poll::Pending,
            Err(err) => Poll::Ready(Err(finchers::error::fail(err))),
        }
    }
}
