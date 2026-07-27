//! HTTP handlers. Kept thin: validate input, delegate to `AppData`, shape
//! the response. All locking lives in `state`, all status codes in `errors`.

use actix_web::{get, post, web, HttpResponse};
use chrono::Local;

use crate::errors::ApiError;
use crate::models::{
    validate_amount, BalanceResponse, ClientBalanceQuery, CreditRequest, DebitRequest,
    NewClientRequest, NewClientResponse, StoreBalancesResponse,
};
use crate::state::AppData;

#[post("/new_client")]
pub async fn new_client(
    data: web::Data<AppData>,
    body: web::Json<NewClientRequest>,
) -> Result<HttpResponse, ApiError> {
    let request = body.into_inner().validated()?;
    let client_id = data.create_client(request)?;
    Ok(HttpResponse::Created().json(NewClientResponse { client_id }))
}

#[post("/new_credit_transaction")]
pub async fn new_credit_transaction(
    data: web::Data<AppData>,
    body: web::Json<CreditRequest>,
) -> Result<HttpResponse, ApiError> {
    let request = body.into_inner();
    validate_amount("credit_amount", request.credit_amount)?;
    let balance = data.credit(request.client_id, request.credit_amount)?;
    Ok(HttpResponse::Ok().json(BalanceResponse {
        client_id: request.client_id,
        balance,
    }))
}

#[post("/new_debit_transaction")]
pub async fn new_debit_transaction(
    data: web::Data<AppData>,
    body: web::Json<DebitRequest>,
) -> Result<HttpResponse, ApiError> {
    let request = body.into_inner();
    validate_amount("debit_amount", request.debit_amount)?;
    let balance = data.debit(request.client_id, request.debit_amount)?;
    Ok(HttpResponse::Ok().json(BalanceResponse {
        client_id: request.client_id,
        balance,
    }))
}

/// Persists all balances to `DDMMYYYY_N.DAT` and resets them to zero.
///
/// Snapshot + reset happen atomically inside the lock; the (slow) file write
/// happens *outside* it so transactions keep flowing during I/O. If the
/// write fails, the drained amounts are credited back so money is never
/// silently lost — debits attempted in that brief window may see a zero
/// balance and be rejected, which is the safe failure mode.
#[post("/store_balances")]
pub async fn store_balances(data: web::Data<AppData>) -> Result<HttpResponse, ApiError> {
    let snapshot = data.drain_balances()?;

    let file_name = format!(
        "{}_{}.DAT",
        Local::now().format("%d%m%Y"),
        data.next_file_sequence()
    );

    let mut contents = String::with_capacity(snapshot.len() * 16);
    for (client_id, balance) in &snapshot {
        contents.push_str(&format!("{client_id} {balance}\n"));
    }

    if let Err(io_error) = tokio::fs::write(&file_name, contents.as_bytes()).await {
        data.restore_balances(&snapshot)?;
        return Err(ApiError::Persistence(io_error));
    }

    Ok(HttpResponse::Ok().json(StoreBalancesResponse {
        file_name,
        clients_persisted: snapshot.len(),
    }))
}

#[get("/client_balance")]
pub async fn client_balance(
    data: web::Data<AppData>,
    query: web::Query<ClientBalanceQuery>,
) -> Result<HttpResponse, ApiError> {
    let client = data.get_client(query.user_id)?;
    Ok(HttpResponse::Ok().json(client))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::StatusCode, test, App};
    use serde_json::{json, Value};

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
}
