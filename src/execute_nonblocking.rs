use finchers::endpoint::wrapper::Wrapper;
use finchers::endpoint::{Context, Endpoint, EndpointResult};
use finchers::error::Error;

use futures::compat::Future01CompatExt;
use futures::future::lazy;
use futures::future::Future;
use futures::ready;
use futures::task;
use futures::task::{JoinHandle, Poll, SpawnExt};

use pin_utils::unsafe_pinned;
use std::pin::PinMut;

use juniper::{GraphQLType, RootNode};
use log::trace;
use std::fmt;
use std::sync::Arc;
use tokio::prelude::future::poll_fn as poll_fn_01;
use tokio_threadpool::{blocking, BlockingError};

use crate::maybe_done::MaybeDone;
use crate::request::{GraphQLResponse, RequestEndpoint};

/// Create a `Wrapper` for building a GraphQL endpoint using the specified `RootNode`.
///
/// The endpoint created by this wrapper will spawn a task which executes the GraphQL query
/// after receiving the request.
pub fn execute_nonblocking<QueryT, MutationT, CtxT>(
    root_node: RootNode<'static, QueryT, MutationT>,
) -> ExecuteNonblocking<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    ExecuteNonblocking {
        root_node,
        use_blocking: true,
    }
}

#[allow(missing_docs)]
pub struct ExecuteNonblocking<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    root_node: RootNode<'static, QueryT, MutationT>,
    use_blocking: bool,
}

impl<QueryT, MutationT, CtxT> fmt::Debug for ExecuteNonblocking<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ExecuteNonblocking").finish()
    }
}

impl<QueryT, MutationT, CtxT> ExecuteNonblocking<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    /// Sets whether to use the Tokio's blocking API when executing the GraphQL query.
    ///
    /// The default value is `true`.
    pub fn use_blocking(self, enabled: bool) -> Self {
        ExecuteNonblocking {
            use_blocking: enabled,
            ..self
        }
    }
}

impl<'a, E, QueryT, MutationT, CtxT> Wrapper<'a, E> for ExecuteNonblocking<QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    type Output = (GraphQLResponse,);
    type Endpoint = ExecuteEndpoint<E, QueryT, MutationT, CtxT>;

    fn wrap(self, endpoint: E) -> Self::Endpoint {
        ExecuteEndpoint {
            context: endpoint,
            request: crate::request::request(),
            root_node: Arc::new(self.root_node),
            use_blocking: self.use_blocking,
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
    root_node: Arc<RootNode<'static, QueryT, MutationT>>,
    use_blocking: bool,
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
            .field("use_blocking", &self.use_blocking)
            .finish()
    }
}

impl<'a, E, QueryT, MutationT, CtxT> Endpoint<'a> for ExecuteEndpoint<E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    type Output = (GraphQLResponse,);
    type Future = ExecuteFuture<'a, E, QueryT, MutationT, CtxT>;

    fn apply(&'a self, cx: &mut Context<'_>) -> EndpointResult<Self::Future> {
        let context = self.context.apply(cx)?;
        let request = self.request.apply(cx)?;
        Ok(ExecuteFuture {
            context: MaybeDone::new(context),
            request: MaybeDone::new(request),
            execute: None,
            endpoint: self,
        })
    }
}

#[derive(Debug)]
pub struct ExecuteFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    context: MaybeDone<E::Future>,
    request: MaybeDone<<RequestEndpoint as Endpoint<'a>>::Future>,
    execute: Option<JoinHandle<Result<GraphQLResponse, BlockingError>>>,
    endpoint: &'a ExecuteEndpoint<E, QueryT, MutationT, CtxT>,
}

impl<'a, E, QueryT, MutationT, CtxT> ExecuteFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    unsafe_pinned!(context: MaybeDone<E::Future>);
    unsafe_pinned!(request: MaybeDone<<RequestEndpoint as Endpoint<'a>>::Future>);
    unsafe_pinned!(execute: Option<JoinHandle<Result<GraphQLResponse, BlockingError>>>);
}

impl<'a, E, QueryT, MutationT, CtxT> Future for ExecuteFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    type Output = Result<(GraphQLResponse,), Error>;

    fn poll(mut self: PinMut<'_, Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        loop {
            if let Some(execute) = self.execute().as_pin_mut() {
                return execute
                    .poll(cx)
                    .map_ok(|response| (response,))
                    .map_err(finchers::error::fail);
            }

            ready!(self.context().poll_ready(cx)?);
            ready!(self.request().poll_ready(cx)?);
            let (context,) = self
                .context()
                .take_ok()
                .expect("The context has already taken");
            let (request,) = self
                .request()
                .take_ok()
                .expect("The request has already taken");
            let root_node = self.endpoint.root_node.clone();

            let handle = if self.endpoint.use_blocking {
                trace!("spawn a GraphQL task (with Tokio's blocking API)");
                let future =
                    poll_fn_01(move || blocking(|| request.execute(&root_node, &context))).compat();
                cx.spawner()
                    .spawn_with_handle(future)
                    .expect("failed to spawn the task for executing a GraphQL query.")
            } else {
                trace!("spawn a GraphQL task");
                let future = lazy(move |_| Ok(request.execute(&root_node, &context)));
                cx.spawner()
                    .spawn_with_handle(future)
                    .expect("failed to spawn the task for executing a GraphQL query.")
            };

            PinMut::set(self.execute(), Some(handle));
        }
    }
}
