// FIXME: remove this feature gate as soon as the rustc version used in docs.rs is updated
#![cfg_attr(finchers_inject_extern_prelude, feature(extern_prelude))]

//! A set of extensions for supporting Juniper integration.

#![doc(html_root_url = "https://docs.rs/finchers-juniper/0.1.0")]
#![warn(
    missing_docs,
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    unused,
)]
// #![warn(rust_2018_compatibility)]
#![cfg_attr(finchers_deny_warnings, deny(warnings))]
#![cfg_attr(finchers_deny_warnings, doc(test(attr(deny(warnings)))))]

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
