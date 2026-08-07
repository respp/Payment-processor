//! Domain model and request/response DTOs, grouped by feature.

// ! FEEDBACK 1: models.rs (un solo archivo) se dividió en models/{client,transaction,balance}.rs
pub mod balance;
pub mod client;
pub mod transaction;

pub use balance::StoreBalancesResponse;
pub use client::{Client, ClientBalanceQuery, NewClientRequest, NewClientResponse};
pub use transaction::{BalanceResponse, CreditRequest, DebitRequest};
