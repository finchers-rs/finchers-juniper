#![feature(rust_2018_preview)]
#![feature(pin, futures_api, arbitrary_self_types)]

//! Endpoints for Juniper integration.

extern crate bytes;
extern crate failure;
extern crate finchers;
extern crate futures;
extern crate http;
extern crate juniper;
extern crate percent_encoding;
extern crate pin_utils;
extern crate serde;
extern crate tokio;
extern crate tokio_threadpool;

mod graphiql;
mod maybe_done;
mod query;
mod request;

pub use crate::graphiql::{graphiql, GraphiQL};
pub use crate::query::{query, Query};
pub use crate::request::{request, GraphQLRequest, GraphQLResponse, RequestEndpoint};
