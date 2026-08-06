//! Credit/debit transaction handlers.

use actix_web::{post, web, HttpResponse};

use crate::errors::ApiError;
use crate::models::{BalanceResponse, CreditRequest, DebitRequest};
use crate::state::AppData;
use crate::validation::Validate;

#[post("/new_credit_transaction")]
pub async fn new_credit_transaction(
    data: web::Data<AppData>,
    body: web::Json<CreditRequest>,
) -> Result<HttpResponse, ApiError> {
    let request = body.into_inner();
    let errors = request.validate();
    // ! FEEDBACK 5: el handler arma el ApiError; validate() ya no conoce ApiError (desacoplado)
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }
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
    let errors = request.validate();
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }
    let balance = data.debit(request.client_id, request.debit_amount)?;
    Ok(HttpResponse::Ok().json(BalanceResponse {
        client_id: request.client_id,
        balance,
    }))
}
