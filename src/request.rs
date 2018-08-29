use finchers::endpoint;
use finchers::endpoint::{Context, Endpoint, EndpointExt, EndpointResult};
use finchers::endpoints::{body, method, path};
use finchers::error;
use finchers::error::Error;
use finchers::input::with_get_cx;

use std::mem::PinMut;
use std::task;
use std::task::Poll;

use futures::future::{Future, TryFuture};
use futures::try_ready;

use juniper;
use juniper::InputValue;

use failure::SyncFailure;
use http::Method;
use percent_encoding::percent_decode;
use serde::Deserialize;

/// Create an endpoint which parses a GraphQL request from the client.
///
/// This endpoint validates if the HTTP method is GET or POST and if the iterator over remaining
/// segments is empty, and skips if the request is not acceptable.
/// If the validation is successed, it will return a Future which behaves as follows:
///
/// * If the method is GET, acquires the query string from the task context and converts it
///   into a value of `GraphQLRequest`.
///   If the query string is missing, it will return an error.
/// * If the method is POST, receives the all contents of the request body and then converts
///   it into a value of `GraphQLRequest`.
pub fn request() -> GraphQLRequest {
    GraphQLRequest {
        inner: method::get(path::end()).or(method::post(path::end())),
    }
}

#[derive(Debug)]
pub struct GraphQLRequest {
    inner: endpoint::Or<method::MatchGet<path::EndPath>, method::MatchPost<path::EndPath>>,
}

impl<'a> Endpoint<'a> for GraphQLRequest {
    type Output = (BatchRequest,);
    type Future = GraphQLRequestFuture<'a>;

    fn apply(&'a self, cx: &mut Context) -> EndpointResult<Self::Future> {
        let _ = self.inner.apply(cx)?;
        if cx.input().method() == Method::GET {
            Ok(GraphQLRequestFuture {
                kind: RequestKind::Get,
            })
        } else {
            Ok(GraphQLRequestFuture {
                kind: RequestKind::Post(body::receive_all().apply(cx)?),
            })
        }
    }
}

#[derive(Debug)]
pub struct GraphQLRequestFuture<'a> {
    kind: RequestKind<'a>,
}

#[derive(Debug)]
enum RequestKind<'a> {
    Get,
    Post(<body::ReceiveAll as Endpoint<'a>>::Future),
}

impl<'a> Future for GraphQLRequestFuture<'a> {
    type Output = Result<(BatchRequest,), Error>;

    fn poll(self: PinMut<Self>, cx: &mut task::Context) -> Poll<Self::Output> {
        let result = match unsafe { PinMut::get_mut_unchecked(self) }.kind {
            RequestKind::Get => with_get_cx(|input| {
                let s = input
                    .uri()
                    .query()
                    .ok_or_else(|| error::bad_request("missing query string"))?;
                BatchRequest::from_query(s)
            }),
            RequestKind::Post(ref mut f) => {
                let (data,) = try_ready!(unsafe { PinMut::new_unchecked(f) }.try_poll(cx));
                with_get_cx(|input| -> Result<_, Error> {
                    match input.content_type().map_err(error::bad_request)? {
                        Some(m) if *m == "application/json" => {
                            serde_json::from_slice(&*data).map_err(error::bad_request)
                        }
                        Some(m) if *m == "application/graphql" => {
                            let query =
                                String::from_utf8(data.to_vec()).map_err(error::bad_request)?;
                            Ok(BatchRequest::Single(juniper::http::GraphQLRequest::new(
                                query, None, None,
                            )))
                        }
                        Some(_m) => Err(error::bad_request("unsupported content-type.")),
                        None => Err(error::bad_request("missing content-type.")),
                    }
                })
            }
        };

        Poll::Ready(result).map_ok(|request| (request,))
    }
}

// ==== BatchRequest ====

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum BatchRequest {
    Single(juniper::http::GraphQLRequest),
    Batch(Vec<juniper::http::GraphQLRequest>),
}

impl BatchRequest {
    pub fn from_query(s: &str) -> Result<BatchRequest, Error> {
        #[derive(Debug, Deserialize)]
        struct ParsedQuery<'a> {
            #[serde(borrow)]
            query: &'a str,
            #[serde(borrow)]
            operation_name: Option<&'a str>,
            #[serde(borrow)]
            variables: Option<&'a str>,
        }

        let parsed: ParsedQuery =
            serde_qs::from_str(s).map_err(|e| error::fail(SyncFailure::new(e)))?;

        let query = percent_decode(parsed.query.as_bytes())
            .decode_utf8()
            .map_err(error::bad_request)?
            .into_owned();

        let operation_name = match parsed.operation_name {
            Some(s) => Some(
                percent_decode(s.as_bytes())
                    .decode_utf8()
                    .map_err(error::bad_request)?
                    .into_owned(),
            ),
            None => None,
        };

        let variables: Option<InputValue> = match parsed.variables {
            Some(variables) => {
                let decoded = percent_decode(variables.as_bytes())
                    .decode_utf8()
                    .map_err(error::bad_request)?;
                serde_json::from_str(&*decoded)
                    .map(Some)
                    .map_err(error::bad_request)?
            }
            None => None,
        };

        Ok(BatchRequest::Single(juniper::http::GraphQLRequest::new(
            query,
            operation_name,
            variables,
        )))
    }
}
