//! Client registration and lookup handlers.

use actix_web::{get, post, web, HttpResponse};

use crate::errors::ApiError;
use crate::models::{ClientBalanceQuery, NewClientRequest, NewClientResponse};
use crate::state::AppData;
use crate::validation::Validate;

#[post("/new_client")]
pub async fn new_client(
    data: web::Data<AppData>,
    body: web::Json<NewClientRequest>,
) -> Result<HttpResponse, ApiError> {
    let request = body.into_inner();
    let errors = request.validate();
    // ! FEEDBACK 5: el handler arma el ApiError; validate() ya no conoce ApiError (desacoplado)
    if !errors.is_empty() {
        return Err(ApiError::Validation(errors));
    }
    let client_id = data.create_client(request.normalized())?;
    Ok(HttpResponse::Created().json(NewClientResponse { client_id }))
}

#[get("/client_balance")]
pub async fn client_balance(
    data: web::Data<AppData>,
    query: web::Query<ClientBalanceQuery>,
) -> Result<HttpResponse, ApiError> {
    let client = data.get_client(query.user_id)?;
    Ok(HttpResponse::Ok().json(client))
}
