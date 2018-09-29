#![cfg_attr(feature = "extern-prelude", feature(extern_prelude))]

//! Endpoints for supporting Juniper integration.

// master
#![doc(html_root_url = "https://finchers-rs.github.io/finchers-juniper")]
// released
//#![doc(html_root_url = "https://finchers-rs.github.io/docs/finchers-juniper/v0.1.0-alpha.3")]
#![warn(
    missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    unused,
)]
// #![warn(rust_2018_compatibility)]
#![cfg_attr(feature = "strict", deny(warnings))]
#![cfg_attr(feature = "strict", doc(test(attr(deny(warnings)))))]

extern crate bytes;
extern crate failure;
extern crate finchers;
#[macro_use]
extern crate futures;
extern crate juniper;
#[macro_use]
extern crate log;
extern crate percent_encoding;
#[macro_use]
extern crate serde;
extern crate http;
extern crate serde_json;
extern crate serde_qs;
extern crate tokio;
extern crate tokio_threadpool;

pub mod execute;
pub mod graphiql;
pub mod request;

pub use graphiql::graphiql_source;
pub use request::graphql_request;
