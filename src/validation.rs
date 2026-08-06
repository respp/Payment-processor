//! Shared validation contract for request DTOs.
//!
//! Deliberately decoupled from `ApiError`/HTTP concerns: implementors only
//! report what's wrong, as plain field-scoped messages. The handler is the
//! only place that knows how to turn "these fields are invalid" into an
//! `ApiError::Validation`, so this module never imports `errors`.

// ! FEEDBACK 5: trait nuevo y único para validar (antes cada DTO tenía su propio criterio y tipo de retorno)
/// Implemented by request DTOs that need input validation.
pub trait Validate {
    /// Returns every problem found, or an empty `Vec` if the value is valid.
    ///
    /// Never short-circuits on the first error: callers collect the full
    /// list, so an API consumer can fix every invalid field in one round
    /// trip instead of discovering them one at a time through repeated
    /// requests. Takes `&self` because checking validity never requires
    /// ownership or mutation — normalizing (e.g. trimming) is a separate
    /// concern, done only after validation succeeds.
    fn validate(&self) -> Vec<String>;
}
