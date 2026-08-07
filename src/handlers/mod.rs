//! HTTP handlers, grouped by feature. Kept thin: validate input, delegate to
//! `AppData`, shape the response. All locking lives in `state`, all status
//! codes in `errors`.

// ! FEEDBACK 1: handlers.rs (un solo archivo) se dividió en handlers/{clients,transactions,balances}.rs
pub mod balances;
pub mod clients;
pub mod transactions;

pub use balances::store_balances;
pub use clients::{client_balance, new_client};
pub use transactions::{new_credit_transaction, new_debit_transaction};

#[cfg(test)]
mod tests {
    use actix_web::{http::StatusCode, test, web, App};
    use serde_json::{json, Value};

    use crate::state::AppData;

    use super::*;

    async fn service() -> impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    > {
        test::init_service(
            App::new()
                .app_data(web::Data::new(AppData::default()))
                .service(new_client)
                .service(new_credit_transaction)
                .service(new_debit_transaction)
                .service(client_balance),
        )
        .await
    }

    #[actix_web::test]
    async fn full_client_lifecycle() {
        let app = service().await;

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/new_client")
                .set_json(json!({
                    "client_name": "Jane Doe",
                    "birth_date": "1990-05-17",
                    "document_number": "30111222",
                    "country": "Argentina"
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["client_id"], 1);

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/new_credit_transaction")
                .set_json(json!({ "client_id": 1, "credit_amount": "150.75" }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["balance"], "150.75");

        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/new_debit_transaction")
                .set_json(json!({ "client_id": 1, "debit_amount": "50.75" }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["balance"], "100.00");

        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/client_balance?user_id=1")
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["client_name"], "Jane Doe");
        assert_eq!(body["balance"], "100.00");
    }

    #[actix_web::test]
    async fn errors_map_to_expected_statuses() {
        let app = service().await;

        // Unknown client → 404.
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/new_credit_transaction")
                .set_json(json!({ "client_id": 42, "credit_amount": "10" }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // Non-positive amount → 400.
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/new_debit_transaction")
                .set_json(json!({ "client_id": 1, "debit_amount": "-5" }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // Duplicate document → 409.
        let client = json!({
            "client_name": "Jane Doe",
            "birth_date": "1990-05-17",
            "document_number": "30111222",
            "country": "Argentina"
        });
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/new_client")
                .set_json(client.clone())
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/new_client")
                .set_json(client)
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[actix_web::test]
    async fn validation_reports_every_invalid_field_in_one_response() {
        let app = service().await;

        // Three independent problems in one request: blank name, blank
        // document, and a birth date in the future. A consumer should be
        // able to fix all three without round-tripping the API three times.
        let resp = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/new_client")
                .set_json(json!({
                    "client_name": "   ",
                    "birth_date": "2999-01-01",
                    "document_number": "   ",
                    "country": "Argentina"
                }))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body: Value = test::read_body_json(resp).await;
        let message = body["message"].as_str().unwrap();
        assert!(message.contains("client_name"));
        assert!(message.contains("document_number"));
        assert!(message.contains("birth_date"));
    }
}
