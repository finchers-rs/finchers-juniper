extern crate finchers;
extern crate finchers_juniper;
extern crate futures; // 0.1
extern crate futures_cpupool;
#[macro_use]
extern crate juniper;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;

use finchers::prelude::*;
use finchers_juniper::execute_with_spawner;

use futures_cpupool::CpuPool;
use juniper::{EmptyMutation, RootNode};

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

    let graphql_endpoint = endpoint::syntax::eos()
        .and(fetch_context)
        .wrap(execute_with_spawner(schema, CpuPool::new_num_cpus()));

    info!("Listening on http://127.0.0.1:4000/");
    finchers::launch(graphql_endpoint).start("127.0.0.1:4000");
}
