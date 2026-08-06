//! Client domain model and its request/response DTOs, including validation.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::validation::Validate;

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
    // ! FEEDBACK 5: normalizar (mut self, trimea) quedó separado de validar (&self, no mutable)
    /// Trims whitespace from every string field. Call only after
    /// [`Validate::validate`] reports no errors — normalizing is a distinct
    /// step from validating, so this method doesn't re-check anything.
    pub fn normalized(mut self) -> Self {
        self.client_name = self.client_name.trim().to_owned();
        self.document_number = self.document_number.trim().to_owned();
        self.country = self.country.trim().to_owned();
        self
    }
}

impl Validate for NewClientRequest {
    // ! FEEDBACK 5: &self en vez de Result<Self, ApiError>; junta todos los errores, no corta en el primero
    /// Rejects blank string fields (after trimming) and a birth date that
    /// is not in the past.
    fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        for (field, value) in [
            ("client_name", &self.client_name),
            ("document_number", &self.document_number),
            ("country", &self.country),
        ] {
            if value.trim().is_empty() {
                errors.push(format!("{field} must not be empty"));
            }
        }

        if self.birth_date >= chrono::Utc::now().date_naive() {
            errors.push("birth_date must be a date in the past".to_owned());
        }

        errors
    }
}

#[derive(Debug, Deserialize)]
pub struct ClientBalanceQuery {
    pub user_id: u32,
}

#[derive(Debug, Serialize)]
pub struct NewClientResponse {
    pub client_id: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_request() -> NewClientRequest {
        NewClientRequest {
            client_name: "  Jane Doe  ".to_owned(),
            birth_date: NaiveDate::from_ymd_opt(1990, 5, 17).expect("valid date"),
            document_number: "X-123".to_owned(),
            country: "AR".to_owned(),
        }
    }

    #[test]
    fn normalized_trims_strings() {
        let req = base_request().normalized();
        assert_eq!(req.client_name, "Jane Doe");
    }

    #[test]
    fn validate_accepts_a_well_formed_request() {
        assert!(base_request().validate().is_empty());
    }

    #[test]
    fn validate_rejects_blank_fields() {
        let mut req = base_request();
        req.document_number = "   ".to_owned();
        let errors = req.validate();
        assert!(errors.iter().any(|e| e.contains("document_number")));
    }

    #[test]
    fn validate_rejects_future_birth_date() {
        let mut req = base_request();
        req.birth_date = chrono::Utc::now().date_naive() + chrono::Days::new(1);
        let errors = req.validate();
        assert!(errors.iter().any(|e| e.contains("birth_date")));
    }

    #[test]
    fn validate_collects_every_error_at_once() {
        let mut req = base_request();
        req.client_name = "   ".to_owned();
        req.document_number = "   ".to_owned();
        req.birth_date = chrono::Utc::now().date_naive() + chrono::Days::new(1);
        // Three independent problems, none masking the others.
        assert_eq!(req.validate().len(), 3);
    }
}
