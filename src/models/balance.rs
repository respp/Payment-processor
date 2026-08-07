//! DTOs for the balance persistence (`store_balances`) endpoint.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct StoreBalancesResponse {
    pub file_name: String,
    pub clients_persisted: usize,
}
