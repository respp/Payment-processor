//! Credit/debit transaction DTOs and shared amount validation.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::validation::Validate;

/// Maximum fractional digits accepted on monetary amounts (whole cents only).
const MAX_AMOUNT_SCALE: u32 = 2;

#[derive(Debug, Deserialize)]
pub struct CreditRequest {
    pub client_id: u32,
    pub credit_amount: Decimal,
}

impl Validate for CreditRequest {
    // ! FEEDBACK 5: mismo trait Validate que NewClientRequest (antes había una función libre suelta)
    fn validate(&self) -> Vec<String> {
        validate_amount("credit_amount", self.credit_amount)
    }
}

#[derive(Debug, Deserialize)]
pub struct DebitRequest {
    pub client_id: u32,
    pub debit_amount: Decimal,
}

impl Validate for DebitRequest {
    fn validate(&self) -> Vec<String> {
        validate_amount("debit_amount", self.debit_amount)
    }
}

/// Shared validation for monetary amounts: strictly positive and at most two
/// decimal places, since fractional cents cannot be settled. Both checks run
/// independently — a value that fails both (e.g. "-1.005") is reported in
/// full instead of only surfacing whichever check happens to run first.
fn validate_amount(field: &str, amount: Decimal) -> Vec<String> {
    let mut errors = Vec::new();
    // ! FEEDBACK 5: las dos validaciones corren siempre; antes se cortaba en la primera que fallaba
    if amount <= Decimal::ZERO {
        errors.push(format!("{field} must be greater than zero"));
    }
    // `normalize` strips trailing zeros so "10.50" (scale 2) and "10.500"
    // (scale 3) are both accepted, while "10.505" is rejected.
    if amount.normalize().scale() > MAX_AMOUNT_SCALE {
        errors.push(format!(
            "{field} must have at most {MAX_AMOUNT_SCALE} decimal places"
        ));
    }
    errors
}

#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    pub client_id: u32,
    pub balance: Decimal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn credit(amount: &str) -> CreditRequest {
        CreditRequest {
            client_id: 1,
            credit_amount: Decimal::from_str(amount).unwrap(),
        }
    }

    #[test]
    fn validate_accepts_positive_amounts_with_cent_precision() {
        assert!(credit("10.50").validate().is_empty());
    }

    #[test]
    fn validate_rejects_non_positive_amounts() {
        assert!(!credit("0").validate().is_empty());
        assert!(!credit("-1").validate().is_empty());
    }

    #[test]
    fn validate_rejects_sub_cent_precision() {
        assert!(!credit("0.005").validate().is_empty());
    }

    #[test]
    fn validate_collects_every_error_at_once() {
        // Negative *and* sub-cent precision: both problems must be reported.
        assert_eq!(credit("-1.005").validate().len(), 2);
    }
}
