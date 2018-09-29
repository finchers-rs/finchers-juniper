use finchers::endpoint;
use finchers::endpoint::wrapper::Wrapper;
use finchers::endpoint::{ApplyContext, ApplyResult, Endpoint, IntoEndpoint};
use finchers::error::Error;

use futures::future;
use futures::sync::oneshot;
use futures::{Async, Future, Poll};
use tokio::executor::{DefaultExecutor, Executor as _TokioExecutor};

use juniper::{GraphQLType, RootNode};
use std::fmt;
use std::sync::Arc;
use tokio_threadpool::{blocking, BlockingError};

use request::{GraphQLRequestEndpoint, GraphQLResponse, RequestFuture};

/// Create a `Wrapper` for building a GraphQL endpoint using the specified `RootNode`.
///
/// The endpoint created by this wrapper will spawn a task which executes the GraphQL query
/// after receiving the request, by using tokio's `DefaultExecutor`.
pub fn nonblocking<QueryT, MutationT, CtxT>(
    root_node: RootNode<'static, QueryT, MutationT>,
) -> Nonblocking<QueryT, MutationT>
where
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    Nonblocking { root_node }
}

#[allow(missing_docs)]
pub struct Nonblocking<QueryT: GraphQLType, MutationT: GraphQLType> {
    root_node: RootNode<'static, QueryT, MutationT>,
}

impl<QueryT, MutationT> fmt::Debug for Nonblocking<QueryT, MutationT>
where
    QueryT: GraphQLType,
    MutationT: GraphQLType,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Nonblocking").finish()
    }
}

impl<'a, QueryT, MutationT> IntoEndpoint<'a> for Nonblocking<QueryT, MutationT>
where
    QueryT: GraphQLType<Context = ()> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = ()> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
{
    type Output = (GraphQLResponse,);
    type Endpoint = NonblockingEndpoint<endpoint::Cloned<()>, QueryT, MutationT>;

    fn into_endpoint(self) -> Self::Endpoint {
        NonblockingEndpoint {
            context: endpoint::cloned(()),
            request: ::request::graphql_request(),
            root_node: Arc::new(self.root_node),
        }
    }
}

impl<'a, E, QueryT, MutationT, CtxT> Wrapper<'a, E> for Nonblocking<QueryT, MutationT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    type Output = (GraphQLResponse,);
    type Endpoint = NonblockingEndpoint<E, QueryT, MutationT>;

    fn wrap(self, endpoint: E) -> Self::Endpoint {
        NonblockingEndpoint {
            context: endpoint,
            request: ::request::graphql_request(),
            root_node: Arc::new(self.root_node),
        }
    }
}

pub struct NonblockingEndpoint<E, QueryT: GraphQLType, MutationT: GraphQLType> {
    context: E,
    request: GraphQLRequestEndpoint,
    root_node: Arc<RootNode<'static, QueryT, MutationT>>,
}

impl<E, QueryT, MutationT> fmt::Debug for NonblockingEndpoint<E, QueryT, MutationT>
where
    E: fmt::Debug,
    QueryT: GraphQLType,
    MutationT: GraphQLType,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExecuteEndpoint")
            .field("context", &self.context)
            .field("request", &self.request)
            .finish()
    }
}

impl<'a, E, QueryT, MutationT, CtxT> Endpoint<'a> for NonblockingEndpoint<E, QueryT, MutationT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    type Output = (GraphQLResponse,);
    type Future = NonblockingFuture<'a, E, QueryT, MutationT, CtxT>;

    fn apply(&'a self, cx: &mut ApplyContext<'_>) -> ApplyResult<Self::Future> {
        let context = self.context.apply(cx)?;
        let request = self.request.apply(cx)?;
        Ok(NonblockingFuture {
            inner: context.join(request),
            handle: None,
            endpoint: self,
        })
    }
}

#[derive(Debug)]
pub struct NonblockingFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: 'a,
{
    inner: future::Join<E::Future, RequestFuture<'a>>,
    handle: Option<JoinHandle<GraphQLResponse, BlockingError>>,
    endpoint: &'a NonblockingEndpoint<E, QueryT, MutationT>,
}

impl<'a, E, QueryT, MutationT, CtxT> Future for NonblockingFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    type Item = (GraphQLResponse,);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match self.handle {
                Some(ref mut handle) => {
                    return handle
                        .poll()
                        .map(|x| x.map(|response| (response,)))
                        .map_err(::finchers::error::fail)
                }
                None => {
                    let ((context,), (request,)) = try_ready!(self.inner.poll());
                    let root_node = self.endpoint.root_node.clone();
                    trace!("spawn a GraphQL task (with Tokio's blocking API)");
                    let future =
                        future::poll_fn(move || blocking(|| request.execute(&root_node, &context)));
                    let handle = spawn_with_handle(future);
                    self.handle = Some(handle);
                }
            }
        }
    }
}

fn spawn_with_handle<F>(future: F) -> JoinHandle<F::Item, F::Error>
where
    F: Future + Send + 'static,
    F::Item: Send + 'static,
    F::Error: Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    let mut tx_opt = Some(tx);
    let mut future = ::std::panic::AssertUnwindSafe(future).catch_unwind();
    let future = future::poll_fn(move || {
        let data = match future.poll() {
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Ok(Async::Ready(res)) => Ok(res),
            Err(panic_err) => Err(panic_err),
        };
        let _ = tx_opt.take().unwrap().send(data);
        Ok(Async::Ready(()))
    });
    DefaultExecutor::current()
        .spawn(Box::new(future))
        .expect("failed to spawn the future");
    JoinHandle { inner: rx }
}

#[derive(Debug)]
struct JoinHandle<T, E> {
    inner: oneshot::Receiver<::std::thread::Result<Result<T, E>>>,
}

impl<T, E> Future for JoinHandle<T, E> {
    type Item = T;
    type Error = E;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(Ok(res))) => res.map(Async::Ready),
            Ok(Async::Ready(Err(panic_err))) => ::std::panic::resume_unwind(panic_err),
            Err(canceled) => ::std::panic::resume_unwind(Box::new(canceled)),
        }
    }
}
