#![feature(rust_2018_preview)]

extern crate finchers;
extern crate finchers_juniper;
extern crate futures;
extern crate juniper;
extern crate percent_encoding;

use std::sync::Arc;

use finchers::prelude::*;
use finchers::endpoint::syntax;
use finchers::local;
use finchers_juniper::{GraphQLRequest, GraphQLResponse};

use juniper::http::tests as http_tests;
use juniper::tests::model::Database;
use juniper::{EmptyMutation, RootNode};

use futures::future::ready;
use percent_encoding::{define_encode_set, utf8_percent_encode, QUERY_ENCODE_SET};

type Schema = RootNode<'static, Database, EmptyMutation<Database>>;

struct TestFinchersIntegration<'e, E: 'e> {
    endpoint: &'e E,
}

impl<'e, E> TestFinchersIntegration<'e, E>
where
    E: Endpoint<'e, Output = (GraphQLResponse,)>,
{
    fn respond_to(&self, req: local::LocalRequest) -> http_tests::TestResponse {
        let response = req.respond(self.endpoint);
        let status_code = response.status().as_u16() as i32;
        let content_type = response
            .headers()
            .get("content-type")
            .expect("No content type header from endpoint")
            .to_str()
            .expect("failed to convert the header value to string")
            .to_owned();
        let body = response.body().to_utf8().into_owned();
        http_tests::TestResponse {
            status_code,
            content_type,
            body: Some(body),
        }
    }
}

fn custom_url_encode(url: &str) -> String {
    define_encode_set!{
        pub CUSTOM_ENCODE_SET = [QUERY_ENCODE_SET] | {'{', '}'}
    }
    utf8_percent_encode(url, CUSTOM_ENCODE_SET).to_string()
}

impl<'e, E> http_tests::HTTPIntegration for TestFinchersIntegration<'e, E>
where
    E: Endpoint<'e, Output = (GraphQLResponse,)>,
{
    fn get(&self, url: &str) -> http_tests::TestResponse {
        let request = local::get(custom_url_encode(url));
        self.respond_to(request)
    }

    fn post(&self, url: &str, body: &str) -> http_tests::TestResponse {
        let request = local::post(custom_url_encode(url))
            .header("content-type", "application/json")
            .body(body.to_owned());
        self.respond_to(request)
    }
}

#[test]
fn test_finchers_integration() {
    let database = Database::new();
    let schema = Schema::new(Database::new(), EmptyMutation::<Database>::new());
    let endpoint = syntax::eos()
        .and(finchers_juniper::request())
        .and(endpoint::value(Arc::new(database)))
        .and(endpoint::value(Arc::new(schema)))
        .and_then(
            |req: GraphQLRequest, db: Arc<Database>, schema: Arc<Schema>| {
                ready(Ok(req.execute(&*schema, &*db)))
            },
        );
    let integration = TestFinchersIntegration {
        endpoint: &endpoint,
    };
    http_tests::run_http_test_suite(&integration);
}
