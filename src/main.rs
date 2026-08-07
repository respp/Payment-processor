mod errors;
mod handlers;
mod models;
mod state;
mod validation;

use actix_web::{middleware::Logger, web, App, HttpServer};

use crate::errors::ApiError;
use crate::state::AppData;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    // One shared state instance; `web::Data` wraps it in an `Arc`, so every
    // worker thread operates on the same clients and counters.
    let data = web::Data::new(AppData::default());

    log::info!("payment processor listening on http://127.0.0.1:8080");

    HttpServer::new(move || {
        // ! FEEDBACK 3: por qué hace falta este .clone() (no es innecesario, ver abajo)
        // `HttpServer::new` calls this factory once per worker thread, and
        // the factory must be callable more than once (`Fn`, not `FnOnce`).
        // Moving `data` in directly would consume it on the first call and
        // fail to compile for any worker count above one, so each call
        // clones it instead. `web::Data<T>` is `Arc<T>` under the hood, so
        // `.clone()` is a refcount bump, not a copy of the client map — every
        // worker ends up holding a handle to the exact same `AppData`.
        App::new()
            .app_data(data.clone())
            // Malformed JSON bodies / query strings would otherwise return
            // actix's plain-text 400; route them through `ApiError` so every
            // error response shares the same JSON shape.
            .app_data(web::JsonConfig::default().error_handler(|err, _req| {
                // ! FEEDBACK 5: ApiError::Validation pasó de String a Vec<String> (acá y en QueryConfig)
                ApiError::Validation(vec![err.to_string()]).into()
            }))
            .app_data(web::QueryConfig::default().error_handler(|err, _req| {
                ApiError::Validation(vec![err.to_string()]).into()
            }))
            .wrap(Logger::default())
            .service(handlers::new_client)
            .service(handlers::new_credit_transaction)
            .service(handlers::new_debit_transaction)
            .service(handlers::store_balances)
            .service(handlers::client_balance)
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
