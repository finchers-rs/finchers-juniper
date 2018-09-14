#![feature(rust_2018_preview)]

extern crate finchers;
extern crate finchers_juniper;
extern crate futures; // 0.3
extern crate futures_cpupool;
extern crate juniper;
extern crate log;
extern crate pretty_env_logger;

use finchers::endpoint::wrapper::with_spawner;
use finchers::prelude::*;
use finchers_juniper::execute_nonblocking;

use futures::compat::Executor01CompatExt;
use futures_cpupool::CpuPool;
use juniper::{graphql_object, EmptyMutation, RootNode};

struct MyContext {
    _priv: (),
}

impl juniper::Context for MyContext {}

struct Query;

graphql_object!(Query: MyContext |&self| {
    field apiVersion() -> &str {
        "1.0"
    }
});

fn main() {
    pretty_env_logger::init();

    let fetch_context = endpoint::unit().map(|| MyContext { _priv: () });
    let schema = RootNode::new(Query, EmptyMutation::<MyContext>::new());

    let pool = CpuPool::new_num_cpus();
    let spawner = pool.compat();

    let graphql_endpoint = endpoint::syntax::eos()
        .and(fetch_context)
        .wrap(execute_nonblocking(schema).use_blocking(false))
        .wrap(with_spawner(spawner));

    log::info!("Listening on http://127.0.0.1:4000/");
    finchers::launch(graphql_endpoint).start("127.0.0.1:4000");
}
