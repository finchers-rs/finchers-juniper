//! Endpoints for supporting Juniper integration.

#![doc(html_root_url = "https://docs.rs/finchers-juniper/0.1.0-alpha.3")]
#![warn(
    missing_docs,
    missing_debug_implementations,
    future_incompatible,
    nonstandard_style,
    rust_2018_idioms,
    unused,
)]
#![cfg_attr(feature = "strict", deny(warnings))]
#![cfg_attr(feature = "strict", doc(test(attr(deny(warnings)))))]

mod execute;
mod execute_nonblocking;
mod graphiql;
mod maybe_done;
mod request;

pub use crate::execute::{execute, Execute};
pub use crate::execute_nonblocking::{execute_nonblocking, ExecuteNonblocking};
pub use crate::graphiql::{graphiql, GraphiQL};
pub use crate::request::{request, GraphQLRequest, GraphQLResponse, RequestEndpoint};
