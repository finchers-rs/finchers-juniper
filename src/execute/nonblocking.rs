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

use super::maybe_done::MaybeDone;
use request::{GraphQLResponse, RequestEndpoint};

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
    Nonblocking {
        root_node,
        use_blocking: true,
    }
}

#[allow(missing_docs)]
pub struct Nonblocking<QueryT: GraphQLType, MutationT: GraphQLType> {
    root_node: RootNode<'static, QueryT, MutationT>,
    use_blocking: bool,
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

impl<QueryT, MutationT> Nonblocking<QueryT, MutationT>
where
    QueryT: GraphQLType,
    MutationT: GraphQLType,
{
    /// Sets whether to use the Tokio's blocking API when executing the GraphQL query.
    ///
    /// The default value is `true`.
    pub fn use_blocking(self, enabled: bool) -> Self {
        Nonblocking {
            use_blocking: enabled,
            ..self
        }
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
            request: ::request::request(),
            root_node: Arc::new(self.root_node),
            use_blocking: self.use_blocking,
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
            request: ::request::request(),
            root_node: Arc::new(self.root_node),
            use_blocking: self.use_blocking,
        }
    }
}

pub struct NonblockingEndpoint<E, QueryT: GraphQLType, MutationT: GraphQLType> {
    context: E,
    request: RequestEndpoint,
    root_node: Arc<RootNode<'static, QueryT, MutationT>>,
    use_blocking: bool,
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
            .field("use_blocking", &self.use_blocking)
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
            context: MaybeDone::new(context),
            request: MaybeDone::new(request),
            execute: None,
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
    context: MaybeDone<E::Future>,
    request: MaybeDone<<RequestEndpoint as Endpoint<'a>>::Future>,
    execute: Option<JoinHandle<GraphQLResponse, BlockingError>>,
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
            if let Some(ref mut execute) = self.execute {
                return execute
                    .poll()
                    .map(|x| x.map(|response| (response,)))
                    .map_err(::finchers::error::fail);
            }

            try_ready!(self.context.poll_ready());
            try_ready!(self.request.poll_ready());
            let (context,) = self
                .context
                .take_ok()
                .expect("The context has already taken");
            let (request,) = self
                .request
                .take_ok()
                .expect("The request has already taken");
            let root_node = self.endpoint.root_node.clone();

            trace!("spawn a GraphQL task (with Tokio's blocking API)");
            let future =
                future::poll_fn(move || blocking(|| request.execute(&root_node, &context)));
            let handle = spawn_with_handle(future);
            self.execute = Some(handle);
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
