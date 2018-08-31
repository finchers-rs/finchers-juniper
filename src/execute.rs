use finchers::endpoint::{Context, Endpoint, EndpointResult};
use finchers::error;
use finchers::error::Error;

use std::pin::PinMut;

use futures::future::Future;
use futures::task;
use futures::task::Poll;
use futures::try_ready;
use pin_utils::unsafe_pinned;

use juniper::{GraphQLType, RootNode};

use tokio::prelude::Async as Async01;
use tokio_threadpool::blocking;

use crate::maybe_done::MaybeDone;
use crate::request::{GraphQLResponse, RequestEndpoint, RequestFuture};

#[allow(missing_docs)]
pub struct Execute<E, QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    pub(super) root_node: RootNode<'static, QueryT, MutationT>,
    pub(super) context_endpoint: E,
    pub(super) request_endpoint: RequestEndpoint,
    pub(super) use_blocking: bool,
}

impl<E, QueryT, MutationT, CtxT> Execute<E, QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    /// Sets whether the endpoint uses tokio's blocking API when executing a GraphQL request or not.
    ///
    /// If the setting is set to `false`, handling of the received GraphQL request will be
    /// immediately executed without calling the tokio's blocking API.
    pub fn use_blocking(self, enabled: bool) -> Self {
        Execute {
            use_blocking: enabled,
            ..self
        }
    }
}

impl<'a, E, QueryT, MutationT, CtxT> Endpoint<'a> for Execute<E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    type Output = (GraphQLResponse,);
    type Future = ExecuteFuture<'a, E, QueryT, MutationT, CtxT>;

    fn apply(&'a self, cx: &mut Context) -> EndpointResult<Self::Future> {
        let request = self.request_endpoint.apply(cx)?;
        let context = self.context_endpoint.apply(cx)?;
        Ok(ExecuteFuture {
            endpoint: self,
            request: MaybeDone::new(request),
            context: MaybeDone::new(context),
        })
    }
}

pub struct ExecuteFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    endpoint: &'a Execute<E, QueryT, MutationT, CtxT>,
    context: MaybeDone<E::Future>,
    request: MaybeDone<RequestFuture<'a>>,
}

impl<'a, E, QueryT, MutationT, CtxT> ExecuteFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    unsafe_pinned!(context: MaybeDone<E::Future>);
    unsafe_pinned!(request: MaybeDone<RequestFuture<'a>>);
}

impl<'a, E, QueryT, MutationT, CtxT> Future for ExecuteFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    type Output = Result<(GraphQLResponse,), Error>;

    fn poll(mut self: PinMut<Self>, cx: &mut task::Context) -> Poll<Self::Output> {
        try_ready!(self.request().poll_ready(cx));
        try_ready!(self.context().poll_ready(cx));
        let response = if self.endpoint.use_blocking {
            match blocking(move || {
                let (request,) = self.request().take_ok().unwrap();
                let (context,) = self.context().take_ok().unwrap();
                request.execute(&self.endpoint.root_node, &context)
            }) {
                Ok(Async01::Ready(response)) => response,
                Ok(Async01::NotReady) => return Poll::Pending,
                Err(err) => return Poll::Ready(Err(error::fail(err))),
            }
        } else {
            let (request,) = self.request().take_ok().unwrap();
            let (context,) = self.context().take_ok().unwrap();
            request.execute(&self.endpoint.root_node, &context)
        };
        Poll::Ready(Ok((response,)))
    }
}
