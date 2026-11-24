//! Application error type and its mapping to HTTP responses.

use actix_web::{http::StatusCode, HttpResponse, ResponseError};
use rust_decimal::Decimal;
use serde::Serialize;
use thiserror::Error;

/// Every failure a handler can produce, each mapped to a stable HTTP status
/// and a machine-readable error code.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("{0}")]
    Validation(String),

    #[error("client {0} does not exist")]
    ClientNotFound(u32),

    #[error("a client with document number '{0}' is already registered")]
    DuplicateDocument(String),

    #[error("insufficient funds: balance is {balance}, debit requested is {requested}")]
    InsufficientFunds {
        balance: Decimal,
        requested: Decimal,
    },

    #[error("failed to persist balances: {0}")]
    Persistence(#[from] std::io::Error),

    /// A thread panicked while holding the state lock. The state may be
    /// inconsistent, so refuse the request instead of panicking as well.
    #[error("internal state is unavailable")]
    LockPoisoned,
}

/// Uniform JSON body returned for every error response.
#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
    message: String,
}

impl ApiError {
    fn code(&self) -> &'static str {
        match self {
            ApiError::Validation(_) => "validation_error",
            ApiError::ClientNotFound(_) => "client_not_found",
            ApiError::DuplicateDocument(_) => "duplicate_document",
            ApiError::InsufficientFunds { .. } => "insufficient_funds",
            ApiError::Persistence(_) => "persistence_error",
            ApiError::LockPoisoned => "internal_error",
        }
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::Validation(_) => StatusCode::BAD_REQUEST,
            ApiError::ClientNotFound(_) => StatusCode::NOT_FOUND,
            ApiError::DuplicateDocument(_) => StatusCode::CONFLICT,
            ApiError::InsufficientFunds { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Persistence(_) | ApiError::LockPoisoned => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).json(ErrorBody {
            error: self.code(),
            message: self.to_string(),
        })
    }
}
