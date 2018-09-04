#![feature(rust_2018_preview)]

extern crate failure;
extern crate finchers;
extern crate finchers_juniper;
extern crate juniper;
extern crate log;
extern crate pretty_env_logger;

use finchers::endpoint;
use finchers::endpoint::EndpointExt;
use finchers::{route, routes};
use finchers_juniper::GraphQLRequest;

use failure::Fallible;
use log::info;
use std::sync::Arc;

use crate::business::Repository;
use crate::graphql::{create_schema, Context};

fn main() -> Fallible<()> {
    pretty_env_logger::try_init()?;

    let repository = Arc::new(Repository::init());
    let schema = Arc::new(create_schema());

    let graphql_endpoint = route!(/ "graphql" /)
        .and(finchers_juniper::request())
        .and(endpoint::value(schema))
        .and(endpoint::value(repository).map(|repository| Context { repository }))
        .and_then(|request: GraphQLRequest, schema, context| {
            request.execute_async(schema, context)
        });

    let graphiql_endpoint = route!(@get /).and(finchers_juniper::graphiql("/graphql"));

    let endpoint = routes![graphql_endpoint, graphiql_endpoint,];

    info!("Listening on http://127.0.0.1:4000");
    finchers::launch(endpoint).start("127.0.0.1:4000");
    Ok(())
}

/// The implelentation of business logic.
mod business {
    use failure::{format_err, Fallible};
    use std::collections::HashMap;
    use std::sync::RwLock;

    #[derive(Debug, Clone)]
    pub struct Todo {
        pub id: i32,
        pub title: String,
        pub text: String,
        pub published: bool,
    }

    #[derive(Debug)]
    pub struct Repository(RwLock<Inner>);

    #[derive(Debug)]
    struct Inner {
        todos: HashMap<i32, Todo>,
        counter: i32,
    }

    impl Repository {
        pub fn init() -> Repository {
            Repository(RwLock::new(Inner {
                todos: HashMap::new(),
                counter: 0,
            }))
        }

        pub fn all_todos(&self) -> Fallible<Vec<Todo>> {
            let inner = self.0.read().map_err(|e| format_err!("{}", e))?;
            Ok(inner.todos.values().cloned().collect())
        }

        pub fn find_todo_by_id(&self, id: i32) -> Fallible<Option<Todo>> {
            let inner = self.0.read().map_err(|e| format_err!("{}", e))?;
            Ok(inner.todos.get(&id).cloned())
        }

        pub fn create_todo(&self, title: String, text: String) -> Fallible<Todo> {
            let mut inner = self.0.write().map_err(|e| format_err!("{}", e))?;

            let new_todo = Todo {
                id: inner.counter,
                title,
                text,
                published: false,
            };
            inner.counter = inner
                .counter
                .checked_add(1)
                .ok_or_else(|| format_err!("overflow detected"))?;
            inner.todos.insert(new_todo.id, new_todo.clone());

            Ok(new_todo)
        }
    }
}

/// The definition of GraphQL schema and resolvers.
mod graphql {
    use juniper;
    use juniper::{graphql_object, FieldResult, RootNode};
    use std::sync::Arc;

    use crate::business::Repository;

    #[derive(Debug)]
    pub struct Context {
        pub repository: Arc<Repository>,
    }

    impl juniper::Context for Context {}

    #[derive(Debug)]
    #[repr(transparent)]
    pub struct Todo(crate::business::Todo);

    graphql_object!(Todo: () |&self| {
        field id() -> i32 { self.0.id }
        field title() -> &String { &self.0.title }
        field text() -> &String { &self.0.text }
        field published() -> bool { self.0.published }
    });

    pub struct Query;

    graphql_object!(Query: Context |&self| {
        field apiVersion() -> &str {
            "1.0"
        }

        field todos(&executor) -> FieldResult<Vec<Todo>> {
            Ok(executor.context()
               .repository
               .all_todos()?
               .into_iter()
               .map(Todo)
               .collect())
        }

        field todo(&executor, id: i32) -> FieldResult<Option<Todo>> {
            Ok(executor.context()
               .repository
               .find_todo_by_id(id)?
               .map(Todo))
        }
    });

    pub struct Mutation;

    graphql_object!(Mutation: Context |&self| {
        field create_todo(&executor, title: String, text: String) -> FieldResult<Todo> {
            executor.context()
               .repository
               .create_todo(title, text)
               .map(Todo)
               .map_err(Into::into)
        }
    });

    pub type Schema = RootNode<'static, Query, Mutation>;

    pub fn create_schema() -> Schema {
        Schema::new(Query, Mutation)
    }
}
