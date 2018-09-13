#![feature(rust_2018_preview)]
#![feature(pin, futures_api, arbitrary_self_types)]

//! Endpoints for supporting Juniper integration.

#![doc(
    html_root_url = "https://docs.rs/finchers-juniper/0.1.0-alpha.2",
    test(attr(feature(rust_2018_preview))),
)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    future_incompatible,
    nonstandard_style,
    rust_2018_idioms,
    unused,
)]
#![allow(keyword_idents)] // See https://github.com/serde-rs/serde/issues/1385
#![cfg_attr(feature = "strict", deny(warnings))]
#![cfg_attr(feature = "strict", doc(test(attr(deny(warnings)))))]

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

mod execute;
mod execute_nonblocking;
mod graphiql;
mod maybe_done;
mod request;

pub use crate::execute::{execute, Execute};
pub use crate::execute_nonblocking::{execute_nonblocking, ExecuteNonblocking};
pub use crate::graphiql::{graphiql, GraphiQL};
pub use crate::request::{request, GraphQLRequest, GraphQLResponse, RequestEndpoint};
