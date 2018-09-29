use finchers::endpoint;
use finchers::endpoint::wrapper::Wrapper;
use finchers::endpoint::{ApplyContext, ApplyResult, Endpoint, IntoEndpoint};
use finchers::error::Error;

use futures::{Future, Poll};

use juniper::{GraphQLType, RootNode};
use std::fmt;
use tokio_threadpool::blocking;

use super::maybe_done::MaybeDone;
use request::{GraphQLResponse, RequestEndpoint};

/// Create a `Wrapper` for building a GraphQL endpoint using the specified `RootNode`.
///
/// The endpoint created by this wrapper will block the current thread
/// to execute the GraphQL query.
pub fn current_thread<QueryT, MutationT, CtxT>(
    root_node: RootNode<'static, QueryT, MutationT>,
) -> CurrentThread<QueryT, MutationT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    CurrentThread {
        root_node,
        use_blocking: true,
    }
}

#[allow(missing_docs)]
pub struct CurrentThread<QueryT: GraphQLType, MutationT: GraphQLType> {
    root_node: RootNode<'static, QueryT, MutationT>,
    use_blocking: bool,
}

impl<QueryT, MutationT> CurrentThread<QueryT, MutationT>
where
    QueryT: GraphQLType,
    MutationT: GraphQLType,
{
    /// Sets whether to use the Tokio's blocking API when executing the GraphQL query.
    ///
    /// The default value is `true`.
    pub fn use_blocking(self, enabled: bool) -> Self {
        CurrentThread {
            use_blocking: enabled,
            ..self
        }
    }
}

impl<QueryT, MutationT> fmt::Debug for CurrentThread<QueryT, MutationT>
where
    QueryT: GraphQLType,
    MutationT: GraphQLType,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Execute")
            .field("use_blocking", &self.use_blocking)
            .finish()
    }
}

impl<'a, QueryT, MutationT> IntoEndpoint<'a> for CurrentThread<QueryT, MutationT>
where
    QueryT: GraphQLType<Context = ()> + 'a,
    MutationT: GraphQLType<Context = ()> + 'a,
{
    type Output = (GraphQLResponse,);
    type Endpoint = CurrentThreadEndpoint<endpoint::Cloned<()>, QueryT, MutationT>;

    fn into_endpoint(self) -> Self::Endpoint {
        CurrentThreadEndpoint {
            context: endpoint::cloned(()),
            request: ::request::request(),
            root_node: self.root_node,
            use_blocking: self.use_blocking,
        }
    }
}

impl<'a, E, QueryT, MutationT, CtxT> Wrapper<'a, E> for CurrentThread<QueryT, MutationT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    type Output = (GraphQLResponse,);
    type Endpoint = CurrentThreadEndpoint<E, QueryT, MutationT>;

    fn wrap(self, endpoint: E) -> Self::Endpoint {
        CurrentThreadEndpoint {
            context: endpoint,
            request: ::request::request(),
            root_node: self.root_node,
            use_blocking: self.use_blocking,
        }
    }
}

pub struct CurrentThreadEndpoint<E, QueryT: GraphQLType, MutationT: GraphQLType> {
    context: E,
    request: RequestEndpoint,
    root_node: RootNode<'static, QueryT, MutationT>,
    use_blocking: bool,
}

impl<E, QueryT, MutationT> fmt::Debug for CurrentThreadEndpoint<E, QueryT, MutationT>
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

impl<'a, E, QueryT, MutationT, CtxT> Endpoint<'a> for CurrentThreadEndpoint<E, QueryT, MutationT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    type Output = (GraphQLResponse,);
    type Future = CurrentThreadFuture<'a, E, QueryT, MutationT, CtxT>;

    fn apply(&'a self, cx: &mut ApplyContext<'_>) -> ApplyResult<Self::Future> {
        let context = self.context.apply(cx)?;
        let request = self.request.apply(cx)?;
        Ok(CurrentThreadFuture {
            context: MaybeDone::new(context),
            request: MaybeDone::new(request),
            endpoint: self,
        })
    }
}

#[derive(Debug)]
pub struct CurrentThreadFuture<'a, E, QueryT, MutationT, CtxT>
where
    E: Endpoint<'a, Output = (CtxT,)>,
    QueryT: GraphQLType<Context = CtxT> + 'a,
    MutationT: GraphQLType<Context = CtxT> + 'a,
    CtxT: 'a,
{
    context: MaybeDone<E::Future>,
    request: MaybeDone<<RequestEndpoint as Endpoint<'a>>::Future>,
    endpoint: &'a CurrentThreadEndpoint<E, QueryT, MutationT>,
}

impl<'a, E, QueryT, MutationT, CtxT> CurrentThreadFuture<'a, E, QueryT, MutationT, CtxT>
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

impl<'a, E, QueryT, MutationT, CtxT> Future for CurrentThreadFuture<'a, E, QueryT, MutationT, CtxT>
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
