use finchers::endpoint::wrapper::Wrapper;
use finchers::endpoint::{Context, Endpoint, EndpointResult};
use finchers::error::Error;

use futures::future;
use futures::future::{ExecuteError, Executor};
use futures::sync::oneshot;
use futures::try_ready;
use futures::{Async, Future, Poll};
use tokio::executor::{DefaultExecutor, Executor as _TokioExecutor};

use juniper::{GraphQLType, RootNode};
use log::trace;
use std::fmt;
use std::sync::Arc;
use tokio_threadpool::{blocking, BlockingError};

use crate::maybe_done::MaybeDone;
use crate::request::{GraphQLResponse, RequestEndpoint};

pub type Task = Box<dyn Future<Item = (), Error = ()> + Send + 'static>;

#[derive(Debug)]
pub struct TokioDefaultExecutor {
    _priv: (),
}

impl Executor<Task> for TokioDefaultExecutor {
    fn execute(&self, future: Task) -> Result<(), ExecuteError<Task>> {
        DefaultExecutor::current()
            .spawn(future)
            .expect("failed to spawn");
        Ok(())
    }
}

/// Create a `Wrapper` for building a GraphQL endpoint using the specified `RootNode`.
///
/// The endpoint created by this wrapper will spawn a task which executes the GraphQL query
/// after receiving the request.
pub fn execute_nonblocking<QueryT, MutationT, CtxT>(
    root_node: RootNode<'static, QueryT, MutationT>,
) -> ExecuteNonblocking<QueryT, MutationT, CtxT, TokioDefaultExecutor>
where
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
{
    ExecuteNonblocking {
        root_node,
        spawner: TokioDefaultExecutor { _priv: () },
        use_blocking: true,
    }
}

#[allow(missing_docs)]
pub struct ExecuteNonblocking<QueryT, MutationT, CtxT, Sp>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    root_node: RootNode<'static, QueryT, MutationT>,
    spawner: Sp,
    use_blocking: bool,
}

impl<QueryT, MutationT, CtxT, Sp> fmt::Debug for ExecuteNonblocking<QueryT, MutationT, CtxT, Sp>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
    Sp: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExecuteNonblocking")
            .field("spawner", &self.spawner)
            .finish()
    }
}

impl<QueryT, MutationT, CtxT, Sp> ExecuteNonblocking<QueryT, MutationT, CtxT, Sp>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    #[doc(hidden)]
    #[deprecated(since = "0.1.0-alpha.4")]
    pub fn use_blocking(self, enabled: bool) -> Self {
        ExecuteNonblocking {
            use_blocking: enabled,
            ..self
        }
    }

    #[allow(missing_docs)]
    pub fn with_spawner<T>(self, spawner: T) -> ExecuteNonblocking<QueryT, MutationT, CtxT, T>
    where
        T: Executor<Task>,
    {
        ExecuteNonblocking {
            spawner,
            use_blocking: false,
            root_node: self.root_node,
        }
    }
}

impl<'a, E, QueryT, MutationT, CtxT, Sp> Wrapper<'a, E>
    for ExecuteNonblocking<QueryT, MutationT, CtxT, Sp>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
    Sp: Executor<Task> + 'a,
{
    type Output = (GraphQLResponse,);
    type Endpoint = ExecuteEndpoint<E, QueryT, MutationT, CtxT, Sp>;

    fn wrap(self, endpoint: E) -> Self::Endpoint {
        ExecuteEndpoint {
            context: endpoint,
            request: crate::request::request(),
            root_node: Arc::new(self.root_node),
            use_blocking: self.use_blocking,
            spawner: self.spawner,
        }
    }
}

pub struct ExecuteEndpoint<E, QueryT, MutationT, CtxT, Sp>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    context: E,
    request: RequestEndpoint,
    root_node: Arc<RootNode<'static, QueryT, MutationT>>,
    use_blocking: bool,
    spawner: Sp,
}

impl<E, QueryT, MutationT, CtxT, Sp> fmt::Debug for ExecuteEndpoint<E, QueryT, MutationT, CtxT, Sp>
where
    E: fmt::Debug,
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
    Sp: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExecuteEndpoint")
            .field("context", &self.context)
            .field("request", &self.request)
            .field("use_blocking", &self.use_blocking)
            .field("spawner", &self.spawner)
            .finish()
    }
}

impl<'a, E, QueryT, MutationT, CtxT, Sp> Endpoint<'a>
    for ExecuteEndpoint<E, QueryT, MutationT, CtxT, Sp>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
    Sp: Executor<Task> + 'a,
{
    type Output = (GraphQLResponse,);
    type Future = ExecuteFuture<'a, E, QueryT, MutationT, CtxT, Sp>;

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
pub struct ExecuteFuture<'a, E, QueryT, MutationT, CtxT, Sp>
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
    execute: Option<JoinHandle<GraphQLResponse, BlockingError>>,
    endpoint: &'a ExecuteEndpoint<E, QueryT, MutationT, CtxT, Sp>,
}

impl<'a, E, QueryT, MutationT, CtxT, Sp> Future
    for ExecuteFuture<'a, E, QueryT, MutationT, CtxT, Sp>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
    Sp: Executor<Task> + 'a,
{
    type Item = (GraphQLResponse,);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            if let Some(ref mut execute) = self.execute {
                return execute
                    .poll()
                    .map(|x| x.map(|response| (response,)))
                    .map_err(finchers::error::fail);
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

            let handle = if self.endpoint.use_blocking {
                trace!("spawn a GraphQL task (with Tokio's blocking API)");
                let future =
                    future::poll_fn(move || blocking(|| request.execute(&root_node, &context)));
                spawn_with_handle(future, &self.endpoint.spawner)
            } else {
                trace!("spawn a GraphQL task");
                let future =
                    future::poll_fn(move || Ok(request.execute(&root_node, &context).into()));
                spawn_with_handle(future, &self.endpoint.spawner)
            };

            self.execute = Some(handle);
        }
    }
}

fn spawn_with_handle<F, Sp>(future: F, spawner: &Sp) -> JoinHandle<F::Item, F::Error>
where
    F: Future + Send + 'static,
    F::Item: Send + 'static,
    F::Error: Send + 'static,
    Sp: Executor<Task>,
{
    let (tx, rx) = oneshot::channel();
    let mut tx_opt = Some(tx);
    let mut future = std::panic::AssertUnwindSafe(future).catch_unwind();
    let future = future::poll_fn(move || {
        let data = match future.poll() {
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Ok(Async::Ready(res)) => Ok(res),
            Err(panic_err) => Err(panic_err),
        };
        let _ = tx_opt.take().unwrap().send(data);
        Ok(Async::Ready(()))
    });
    spawner
        .execute(Box::new(future))
        .expect("failed to spawn the future");
    JoinHandle { inner: rx }
}

#[derive(Debug)]
struct JoinHandle<T, E> {
    inner: oneshot::Receiver<std::thread::Result<Result<T, E>>>,
}

impl<T, E> Future for JoinHandle<T, E> {
    type Item = T;
    type Error = E;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(Ok(res))) => res.map(Async::Ready),
            Ok(Async::Ready(Err(panic_err))) => std::panic::resume_unwind(panic_err),
            Err(canceled) => std::panic::resume_unwind(Box::new(canceled)),
        }
    }
}
