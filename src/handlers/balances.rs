//! Balance snapshot/persistence handler.

use actix_web::{post, web, HttpResponse};
use chrono::Local;

use crate::errors::ApiError;
use crate::models::StoreBalancesResponse;
use crate::state::AppData;

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
