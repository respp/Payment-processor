//! Domain model and request/response DTOs, including input validation.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::errors::ApiError;

/// Maximum fractional digits accepted on monetary amounts (whole cents only).
const MAX_AMOUNT_SCALE: u32 = 2;

/// A registered client. Serialized as-is by `GET /client_balance`.
#[derive(Debug, Clone, Serialize)]
pub struct Client {
    pub client_id: u32,
    pub client_name: String,
    pub birth_date: NaiveDate,
    pub document_number: String,
    pub country: String,
    pub balance: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct NewClientRequest {
    pub client_name: String,
    pub birth_date: NaiveDate,
    pub document_number: String,
    pub country: String,
}

impl NewClientRequest {
    /// Normalizes (trims) all string fields and rejects empty values or a
    /// birth date that is not in the past. Consuming `self` and returning the
    /// normalized request guarantees callers can only store validated data.
    pub fn validated(mut self) -> Result<Self, ApiError> {
        self.client_name = self.client_name.trim().to_owned();
        self.document_number = self.document_number.trim().to_owned();
        self.country = self.country.trim().to_owned();

        for (field, value) in [
            ("client_name", &self.client_name),
            ("document_number", &self.document_number),
            ("country", &self.country),
        ] {
            if value.is_empty() {
                return Err(ApiError::Validation(format!("{field} must not be empty")));
            }
        }

        if self.birth_date >= chrono::Utc::now().date_naive() {
            return Err(ApiError::Validation(
                "birth_date must be a date in the past".to_owned(),
            ));
        }

        Ok(self)
    }
}

#[derive(Debug, Deserialize)]
pub struct CreditRequest {
    pub client_id: u32,
    pub credit_amount: Decimal,
}

#[derive(Debug, Deserialize)]
pub struct DebitRequest {
    pub client_id: u32,
    pub debit_amount: Decimal,
}

/// Shared validation for monetary amounts: strictly positive and at most two
/// decimal places, since fractional cents cannot be settled.
pub fn validate_amount(field: &str, amount: Decimal) -> Result<(), ApiError> {
    if amount <= Decimal::ZERO {
        return Err(ApiError::Validation(format!(
            "{field} must be greater than zero"
        )));
    }
    // `normalize` strips trailing zeros so "10.50" (scale 2) and "10.500"
    // (scale 3) are both accepted, while "10.505" is rejected.
    if amount.normalize().scale() > MAX_AMOUNT_SCALE {
        return Err(ApiError::Validation(format!(
            "{field} must have at most {MAX_AMOUNT_SCALE} decimal places"
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct ClientBalanceQuery {
    pub user_id: u32,
}

#[derive(Debug, Serialize)]
pub struct NewClientResponse {
    pub client_id: u32,
}

#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    pub client_id: u32,
    pub balance: Decimal,
}

#[derive(Debug, Serialize)]
pub struct StoreBalancesResponse {
    pub file_name: String,
    pub clients_persisted: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn base_request() -> NewClientRequest {
        NewClientRequest {
            client_name: "  Jane Doe  ".to_owned(),
            birth_date: NaiveDate::from_ymd_opt(1990, 5, 17).expect("valid date"),
            document_number: "X-123".to_owned(),
            country: "AR".to_owned(),
        }
    }

    #[test]
    fn validated_trims_strings() {
        let req = base_request().validated().expect("should be valid");
        assert_eq!(req.client_name, "Jane Doe");
    }

    #[test]
    fn validated_rejects_blank_fields() {
        let mut req = base_request();
        req.document_number = "   ".to_owned();
        assert!(matches!(req.validated(), Err(ApiError::Validation(_))));
    }

    #[test]
    fn validated_rejects_future_birth_date() {
        let mut req = base_request();
        req.birth_date = chrono::Utc::now().date_naive() + chrono::Days::new(1);
        assert!(matches!(req.validated(), Err(ApiError::Validation(_))));
    }

    #[test]
    fn amounts_must_be_positive_with_cent_precision() {
        assert!(validate_amount("credit_amount", Decimal::from_str("10.50").unwrap()).is_ok());
        assert!(validate_amount("credit_amount", Decimal::ZERO).is_err());
        assert!(validate_amount("credit_amount", Decimal::from_str("-1").unwrap()).is_err());
        assert!(validate_amount("credit_amount", Decimal::from_str("0.005").unwrap()).is_err());
    }
}
