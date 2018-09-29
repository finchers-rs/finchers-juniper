use finchers::endpoint;
use finchers::endpoint::wrapper::Wrapper;
use finchers::endpoint::{ApplyContext, ApplyResult, Endpoint, IntoEndpoint};
use finchers::error::Error;

use futures::future;
use futures::future::Executor;
use futures::sync::oneshot;
use futures::{Async, Future, Poll};

use juniper::{GraphQLType, RootNode};
use std::fmt;
use std::sync::Arc;

use request::{GraphQLRequestEndpoint, GraphQLResponse, RequestFuture};

type Task = Box<dyn Future<Item = (), Error = ()> + Send + 'static>;

/// Create a `Wrapper` for building a GraphQL endpoint using the specified `RootNode`.
///
/// The endpoint created by this wrapper will spawn a task which executes the GraphQL query
/// after receiving the request, by using the specified `Executor<T>`.
pub fn with_spawner<QueryT, MutationT, CtxT, Sp>(
    root_node: RootNode<'static, QueryT, MutationT>,
    spawner: Sp,
) -> WithSpawner<QueryT, MutationT, Sp>
where
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
    Sp: Executor<Task>,
{
    WithSpawner { root_node, spawner }
}

#[allow(missing_docs)]
pub struct WithSpawner<QueryT: GraphQLType, MutationT: GraphQLType, Sp> {
    root_node: RootNode<'static, QueryT, MutationT>,
    spawner: Sp,
}

impl<QueryT, MutationT, Sp> fmt::Debug for WithSpawner<QueryT, MutationT, Sp>
where
    QueryT: GraphQLType,
    MutationT: GraphQLType,
    Sp: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WithSpawner")
            .field("spawner", &self.spawner)
            .finish()
    }
}

impl<'a, QueryT, MutationT, Sp> IntoEndpoint<'a> for WithSpawner<QueryT, MutationT, Sp>
where
    QueryT: GraphQLType<Context = ()> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = ()> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    Sp: Executor<Task> + 'a,
{
    type Output = (GraphQLResponse,);
    type Endpoint = WithSpawnerEndpoint<endpoint::Cloned<()>, QueryT, MutationT, Sp>;

    fn into_endpoint(self) -> Self::Endpoint {
        WithSpawnerEndpoint {
            context: endpoint::cloned(()),
            request: ::request::graphql_request(),
            root_node: Arc::new(self.root_node),
            spawner: self.spawner,
        }
    }
}

impl<'a, E, QueryT, MutationT, CtxT, Sp> Wrapper<'a, E> for WithSpawner<QueryT, MutationT, Sp>
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
    type Endpoint = WithSpawnerEndpoint<E, QueryT, MutationT, Sp>;

    fn wrap(self, endpoint: E) -> Self::Endpoint {
        WithSpawnerEndpoint {
            context: endpoint,
            request: ::request::graphql_request(),
            root_node: Arc::new(self.root_node),
            spawner: self.spawner,
        }
    }
}

pub struct WithSpawnerEndpoint<E, QueryT: GraphQLType, MutationT: GraphQLType, Sp> {
    context: E,
    request: GraphQLRequestEndpoint,
    root_node: Arc<RootNode<'static, QueryT, MutationT>>,
    spawner: Sp,
}

impl<E, QueryT, MutationT, Sp> fmt::Debug for WithSpawnerEndpoint<E, QueryT, MutationT, Sp>
where
    E: fmt::Debug,
    QueryT: GraphQLType,
    MutationT: GraphQLType,
    Sp: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExecuteEndpoint")
            .field("context", &self.context)
            .field("request", &self.request)
            .field("spawner", &self.spawner)
            .finish()
    }
}

impl<'a, E, QueryT, MutationT, CtxT, Sp> Endpoint<'a>
    for WithSpawnerEndpoint<E, QueryT, MutationT, Sp>
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
    type Future = WithSpawnerFuture<'a, E, QueryT, MutationT, CtxT, Sp>;

    fn apply(&'a self, cx: &mut ApplyContext<'_>) -> ApplyResult<Self::Future> {
        let context = self.context.apply(cx)?;
        let request = self.request.apply(cx)?;
        Ok(WithSpawnerFuture {
            inner: context.join(request),
            handle: None,
            endpoint: self,
        })
    }
}

#[derive(Debug)]
pub struct WithSpawnerFuture<'a, E, QueryT, MutationT, CtxT, Sp>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    QueryT::TypeInfo: Send + Sync + 'static,
    MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
    MutationT::TypeInfo: Send + Sync + 'static,
    CtxT: Send + 'static,
    Sp: 'a,
{
    inner: future::Join<E::Future, RequestFuture<'a>>,
    handle: Option<JoinHandle<GraphQLResponse, ()>>,
    endpoint: &'a WithSpawnerEndpoint<E, QueryT, MutationT, Sp>,
}

impl<'a, E, QueryT, MutationT, CtxT, Sp> Future
    for WithSpawnerFuture<'a, E, QueryT, MutationT, CtxT, Sp>
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
            match self.handle {
                Some(ref mut handle) => {
                    return handle
                        .poll()
                        .map(|x| x.map(|response| (response,)))
                        .map_err(|_| unreachable!());
                }
                None => {
                    trace!("spawn a GraphQL task");
                    let ((context,), (request,)) = try_ready!(self.inner.poll());
                    let root_node = self.endpoint.root_node.clone();
                    let future =
                        future::poll_fn(move || Ok(request.execute(&root_node, &context).into()));
                    let handle = spawn_with_handle(future, &self.endpoint.spawner);
                    self.handle = Some(handle);
                }
            }
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
    spawner
        .execute(Box::new(future))
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
