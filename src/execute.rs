use finchers::endpoint::wrapper::Wrapper;
use finchers::endpoint::{ApplyContext, ApplyResult, Endpoint};
use finchers::error::Error;

use futures::{Future, Poll};

use juniper::{GraphQLType, RootNode};
use std::fmt;
use tokio_threadpool::blocking;

use maybe_done::MaybeDone;
use request::{GraphQLResponse, RequestEndpoint};

/// Create a `Wrapper` for building a GraphQL endpoint using the specified `RootNode`.
///
/// The endpoint created by this wrapper will block the current thread
/// to execute the GraphQL query.
pub fn execute<QueryT, MutationT, CtxT>(
    root_node: RootNode<'static, QueryT, MutationT>,
) -> Execute<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    Execute {
        root_node,
        use_blocking: true,
    }
}

#[allow(missing_docs)]
pub struct Execute<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    root_node: RootNode<'static, QueryT, MutationT>,
    use_blocking: bool,
}

impl<QueryT, MutationT, CtxT> Execute<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    /// Sets whether to use the Tokio's blocking API when executing the GraphQL query.
    ///
    /// The default value is `true`.
    pub fn use_blocking(self, enabled: bool) -> Self {
        Execute {
            use_blocking: enabled,
            ..self
        }
    }
}

impl<QueryT, MutationT, CtxT> fmt::Debug for Execute<QueryT, MutationT, CtxT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Execute")
            .field("use_blocking", &self.use_blocking)
            .finish()
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
            request: ::request::request(),
            root_node: self.root_node,
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
    root_node: RootNode<'static, QueryT, MutationT>,
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
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    type Output = (GraphQLResponse,);
    type Future = ExecuteFuture<'a, E, QueryT, MutationT, CtxT>;

    fn apply(&'a self, cx: &mut ApplyContext<'_>) -> ApplyResult<Self::Future> {
        let context = self.context.apply(cx)?;
        let request = self.request.apply(cx)?;
        Ok(ExecuteFuture {
            context: MaybeDone::new(context),
            request: MaybeDone::new(request),
            endpoint: self,
        })
    }
}

#[derive(Debug)]
pub struct ExecuteFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    context: MaybeDone<E::Future>,
    request: MaybeDone<<RequestEndpoint as Endpoint<'a>>::Future>,
    endpoint: &'a ExecuteEndpoint<E, QueryT, MutationT, CtxT>,
}

impl<'a, E, QueryT, MutationT, CtxT> ExecuteFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
    CtxT: 'a,
{
    fn execute(&mut self) -> GraphQLResponse {
        let (context,) = self
            .context
            .take_ok()
            .expect("The context has already taken");
        let (request,) = self
            .request
            .take_ok()
            .expect("The request has already taken");
        request.execute(&self.endpoint.root_node, &context)
    }
}

impl<'a, E, QueryT, MutationT, CtxT> Future for ExecuteFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
    CtxT: 'a,
{
    type Item = (GraphQLResponse,);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        try_ready!(self.context.poll_ready());
        try_ready!(self.request.poll_ready());
        if self.endpoint.use_blocking {
            blocking(move || self.execute())
                .map(|x| x.map(|response| (response,)))
                .map_err(::finchers::error::fail)
        } else {
            let response = self.execute();
            Ok((response,).into())
        }
    }
}

pub(crate) mod nonblocking {
    use finchers::endpoint::wrapper::Wrapper;
    use finchers::endpoint::{ApplyContext, ApplyResult, Endpoint};
    use finchers::error::Error;

    use futures::future;
    use futures::sync::oneshot;
    use futures::{Async, Future, Poll};
    use tokio::executor::{DefaultExecutor, Executor as _TokioExecutor};

    use juniper::{GraphQLType, RootNode};
    use std::fmt;
    use std::sync::Arc;
    use tokio_threadpool::{blocking, BlockingError};

    use maybe_done::MaybeDone;
    use request::{GraphQLResponse, RequestEndpoint};

    /// Create a `Wrapper` for building a GraphQL endpoint using the specified `RootNode`.
    ///
    /// The endpoint created by this wrapper will spawn a task which executes the GraphQL query
    /// after receiving the request, by using tokio's `DefaultExecutor`.
    pub fn execute<QueryT, MutationT, CtxT>(
        root_node: RootNode<'static, QueryT, MutationT>,
    ) -> Execute<QueryT, MutationT, CtxT>
    where
        QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
        QueryT::TypeInfo: Send + Sync + 'static,
        MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
        MutationT::TypeInfo: Send + Sync + 'static,
        CtxT: Send + 'static,
    {
        Execute {
            root_node,
            use_blocking: true,
        }
    }

    #[allow(missing_docs)]
    pub struct Execute<QueryT, MutationT, CtxT>
    where
        QueryT: GraphQLType<Context = CtxT>,
        MutationT: GraphQLType<Context = CtxT>,
    {
        root_node: RootNode<'static, QueryT, MutationT>,
        use_blocking: bool,
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

    impl<QueryT, MutationT, CtxT> Execute<QueryT, MutationT, CtxT>
    where
        QueryT: GraphQLType<Context = CtxT>,
        MutationT: GraphQLType<Context = CtxT>,
    {
        /// Sets whether to use the Tokio's blocking API when executing the GraphQL query.
        ///
        /// The default value is `true`.
        pub fn use_blocking(self, enabled: bool) -> Self {
            Execute {
                use_blocking: enabled,
                ..self
            }
        }
    }

    impl<'a, E, QueryT, MutationT, CtxT> Wrapper<'a, E> for Execute<QueryT, MutationT, CtxT>
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
                request: ::request::request(),
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

        fn apply(&'a self, cx: &mut ApplyContext<'_>) -> ApplyResult<Self::Future> {
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
        CtxT: 'a,
    {
        context: MaybeDone<E::Future>,
        request: MaybeDone<<RequestEndpoint as Endpoint<'a>>::Future>,
        execute: Option<JoinHandle<GraphQLResponse, BlockingError>>,
        endpoint: &'a ExecuteEndpoint<E, QueryT, MutationT, CtxT>,
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
}

pub(crate) mod with_spawner {
    use finchers::endpoint::wrapper::Wrapper;
    use finchers::endpoint::{ApplyContext, ApplyResult, Endpoint};
    use finchers::error::Error;

    use futures::future;
    use futures::future::Executor;
    use futures::sync::oneshot;
    use futures::{Async, Future, Poll};

    use juniper::{GraphQLType, RootNode};
    use std::fmt;
    use std::sync::Arc;

    use maybe_done::MaybeDone;
    use request::{GraphQLResponse, RequestEndpoint};

    type Task = Box<dyn Future<Item = (), Error = ()> + Send + 'static>;

    /// Create a `Wrapper` for building a GraphQL endpoint using the specified `RootNode`.
    ///
    /// The endpoint created by this wrapper will spawn a task which executes the GraphQL query
    /// after receiving the request, by using the specified `Executor<T>`.
    pub fn execute<QueryT, MutationT, CtxT, Sp>(
        root_node: RootNode<'static, QueryT, MutationT>,
        spawner: Sp,
    ) -> Execute<QueryT, MutationT, CtxT, Sp>
    where
        QueryT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
        QueryT::TypeInfo: Send + Sync + 'static,
        MutationT: GraphQLType<Context = CtxT> + Send + Sync + 'static,
        MutationT::TypeInfo: Send + Sync + 'static,
        CtxT: Send + 'static,
        Sp: Executor<Task>,
    {
        Execute { root_node, spawner }
    }

    #[allow(missing_docs)]
    pub struct Execute<QueryT, MutationT, CtxT, Sp>
    where
        QueryT: GraphQLType<Context = CtxT>,
        MutationT: GraphQLType<Context = CtxT>,
    {
        root_node: RootNode<'static, QueryT, MutationT>,
        spawner: Sp,
    }

    impl<QueryT, MutationT, CtxT, Sp> fmt::Debug for Execute<QueryT, MutationT, CtxT, Sp>
    where
        QueryT: GraphQLType<Context = CtxT>,
        MutationT: GraphQLType<Context = CtxT>,
        Sp: fmt::Debug,
    {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("Execute")
                .field("spawner", &self.spawner)
                .finish()
        }
    }

    impl<'a, E, QueryT, MutationT, CtxT, Sp> Wrapper<'a, E> for Execute<QueryT, MutationT, CtxT, Sp>
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
                request: ::request::request(),
                root_node: Arc::new(self.root_node),
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

        fn apply(&'a self, cx: &mut ApplyContext<'_>) -> ApplyResult<Self::Future> {
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
        Sp: 'a,
    {
        context: MaybeDone<E::Future>,
        request: MaybeDone<<RequestEndpoint as Endpoint<'a>>::Future>,
        execute: Option<JoinHandle<GraphQLResponse, ()>>,
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
                        .map_err(|_| unreachable!());
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

                trace!("spawn a GraphQL task");
                let future =
                    future::poll_fn(move || Ok(request.execute(&root_node, &context).into()));
                let handle = spawn_with_handle(future, &self.endpoint.spawner);
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
}
