use finchers::endpoint;
use finchers::endpoint::wrapper::Wrapper;
use finchers::endpoint::{ApplyContext, ApplyResult, Endpoint, IntoEndpoint};
use finchers::error::Error;

use futures::future;
use futures::{Future, Poll};

use juniper::{GraphQLType, RootNode};
use std::fmt;

use request::{GraphQLRequestEndpoint, GraphQLResponse, RequestFuture};

/// Create a GraphQL executor from the specified `RootNode`.
///
/// The endpoint created by this executor will execute the GraphQL queries
/// on the current thread.
pub fn current_thread<QueryT, MutationT, CtxT>(
    root_node: RootNode<'static, QueryT, MutationT>,
) -> CurrentThread<QueryT, MutationT>
where
    QueryT: GraphQLType<Context = CtxT>,
    MutationT: GraphQLType<Context = CtxT>,
{
    CurrentThread { root_node }
}

#[allow(missing_docs)]
pub struct CurrentThread<QueryT: GraphQLType, MutationT: GraphQLType> {
    root_node: RootNode<'static, QueryT, MutationT>,
}

impl<QueryT, MutationT> fmt::Debug for CurrentThread<QueryT, MutationT>
where
    QueryT: GraphQLType,
    MutationT: GraphQLType,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("CurrentThread").finish()
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
            request: ::request::graphql_request(),
            root_node: self.root_node,
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
            request: ::request::graphql_request(),
            root_node: self.root_node,
        }
    }
}

pub struct CurrentThreadEndpoint<E, QueryT: GraphQLType, MutationT: GraphQLType> {
    context: E,
    request: GraphQLRequestEndpoint,
    root_node: RootNode<'static, QueryT, MutationT>,
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
            inner: context.join(request),
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
    inner: future::Join<E::Future, RequestFuture<'a>>,
    endpoint: &'a CurrentThreadEndpoint<E, QueryT, MutationT>,
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
        let ((context,), (request,)) = try_ready!(self.inner.poll());
        let response = request.execute(&self.endpoint.root_node, &context);
        Ok((response,).into())
    }
}
