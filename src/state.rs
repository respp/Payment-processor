//! Shared in-memory state and all operations that mutate it.
//!
//! Handlers never touch the lock directly — every state transition lives
//! here, so the locking discipline can be audited in one file.

use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use rust_decimal::Decimal;

use crate::errors::ApiError;
use crate::models::{Client, NewClientRequest};

/// The client registry: every registered client, plus the index that
/// enforces document-number uniqueness. Guarded by the single `RwLock` in
/// [`AppData`].
///
/// `clients` and `documents` deliberately share one lock instead of two.
/// The only place that touches `documents` at all — for reads *or* writes —
/// is [`AppData::create_client`], and it always touches both fields
/// together: the uniqueness check and the client insert must happen under
/// one guard, or two concurrent registrations with the same document number
/// could both pass the check before either commits (a TOCTOU race). Splitting
/// them into separate locks wouldn't remove any real contention — nothing
/// else reads or writes `documents`, so `create_client` would still need to
/// hold both locks for the same duration to keep that guarantee — while
/// adding a second lock to order and reason about on every registration.
/// One lock, one invariant to audit.
// ! FEEDBACK 6: renombrado desde AppState (nombre demasiado genérico) a ClientRegistry
#[derive(Default)]
pub struct ClientRegistry {
    clients: HashMap<u32, Client>,
    /// Secondary index enforcing document-number uniqueness in O(1).
    /// Invariant: contains exactly the document numbers present in `clients`.
    documents: HashSet<String>,
}

/// Shared handle stored in `web::Data` (an `Arc<AppData>`), giving every
/// worker thread the required `Arc<RwLock<ClientRegistry>>` view of the
/// state.
///
/// Design notes:
/// - The counters live *outside* the lock, as their own `AtomicU32` fields:
///   they only need atomicity, not exclusive access to the registry.
///   `Relaxed` ordering suffices since uniqueness — not cross-thread
///   happens-before — is the only guarantee we rely on.
/// - This is `std::sync::RwLock`, not tokio's: no guard is ever held across
///   an `.await`, and the critical sections are short and CPU-only, so a
///   brief blocking acquire is cheaper than async lock bookkeeping.
// ! FEEDBACK 7: no se cambió a tokio::sync::RwLock — std::sync es lo correcto acá (ver doc arriba)
pub struct AppData {
    next_client_id: AtomicU32,
    file_sequence: AtomicU32,
    registry: RwLock<ClientRegistry>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            next_client_id: AtomicU32::new(1),
            file_sequence: AtomicU32::new(1),
            registry: RwLock::new(ClientRegistry::default()),
        }
    }
}

impl AppData {
    /// Lock acquisition returns `Err` instead of panicking if a previous
    /// holder panicked (poisoned lock), keeping unwinds out of handlers.
    fn read(&self) -> Result<RwLockReadGuard<'_, ClientRegistry>, ApiError> {
        self.registry.read().map_err(|_| ApiError::LockPoisoned)
    }

    fn write(&self) -> Result<RwLockWriteGuard<'_, ClientRegistry>, ApiError> {
        self.registry.write().map_err(|_| ApiError::LockPoisoned)
    }

    /// Registers a client and returns its generated ID.
    ///
    /// The uniqueness check and the insert happen under one write guard, so
    /// two concurrent requests with the same document number cannot both
    /// pass the check. The ID is allocated only after the check succeeds,
    /// so rejected registrations never consume IDs.
    pub fn create_client(&self, req: NewClientRequest) -> Result<u32, ApiError> {
        let mut registry = self.write()?;

        if registry.documents.contains(&req.document_number) {
            return Err(ApiError::DuplicateDocument(req.document_number));
        }

        let client_id = self.next_client_id.fetch_add(1, Ordering::Relaxed);
        registry.documents.insert(req.document_number.clone());
        registry.clients.insert(
            client_id,
            Client {
                client_id,
                client_name: req.client_name,
                birth_date: req.birth_date,
                document_number: req.document_number,
                country: req.country,
                balance: Decimal::ZERO,
            },
        );
        Ok(client_id)
    }

    /// Adds `amount` to the client's balance and returns the new balance.
    pub fn credit(&self, client_id: u32, amount: Decimal) -> Result<Decimal, ApiError> {
        let mut registry = self.write()?;
        let client = registry
            .clients
            .get_mut(&client_id)
            .ok_or(ApiError::ClientNotFound(client_id))?;
        client.balance += amount;
        Ok(client.balance)
    }

    /// Subtracts `amount` from the client's balance and returns the new
    /// balance. Overdrafts are rejected; check and subtraction share one
    /// write guard so concurrent debits cannot both pass the funds check.
    pub fn debit(&self, client_id: u32, amount: Decimal) -> Result<Decimal, ApiError> {
        let mut registry = self.write()?;
        let client = registry
            .clients
            .get_mut(&client_id)
            .ok_or(ApiError::ClientNotFound(client_id))?;
        if client.balance < amount {
            return Err(ApiError::InsufficientFunds {
                balance: client.balance,
                requested: amount,
            });
        }
        client.balance -= amount;
        Ok(client.balance)
    }

    pub fn get_client(&self, client_id: u32) -> Result<Client, ApiError> {
        self.read()?
            .clients
            .get(&client_id)
            .cloned()
            .ok_or(ApiError::ClientNotFound(client_id))
    }

    /// Captures every balance and resets them to zero in a single critical
    /// section, so no transaction can slip in between snapshot and reset.
    /// Returns the snapshot sorted by client ID for deterministic output.
    pub fn drain_balances(&self) -> Result<Vec<(u32, Decimal)>, ApiError> {
        let mut registry = self.write()?;
        let mut snapshot: Vec<(u32, Decimal)> = registry
            .clients
            .values_mut()
            .map(|client| (client.client_id, mem::take(&mut client.balance)))
            .collect();
        snapshot.sort_unstable_by_key(|(client_id, _)| *client_id);
        Ok(snapshot)
    }

    /// Compensates a failed persistence attempt by crediting each drained
    /// amount back. Amounts are *added* rather than overwritten so that
    /// transactions applied after the drain are preserved.
    pub fn restore_balances(&self, snapshot: &[(u32, Decimal)]) -> Result<(), ApiError> {
        let mut registry = self.write()?;
        for (client_id, amount) in snapshot {
            if let Some(client) = registry.clients.get_mut(client_id) {
                client.balance += *amount;
            }
        }
        Ok(())
    }

    /// Next sequence number for balance files; unique for the lifetime of
    /// the process, as the spec requires ("since service start").
    pub fn next_file_sequence(&self) -> u32 {
        self.file_sequence.fetch_add(1, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).expect("valid decimal literal")
    }

    fn request(document_number: &str) -> NewClientRequest {
        NewClientRequest {
            client_name: "Jane Doe".to_owned(),
            birth_date: NaiveDate::from_ymd_opt(1990, 5, 17).expect("valid date"),
            document_number: document_number.to_owned(),
            country: "AR".to_owned(),
        }
    }

    #[test]
    fn client_ids_start_at_one_and_increment() {
        let data = AppData::default();
        assert_eq!(data.create_client(request("A")).unwrap(), 1);
        assert_eq!(data.create_client(request("B")).unwrap(), 2);
    }

    #[test]
    fn duplicate_document_is_rejected_and_burns_no_id() {
        let data = AppData::default();
        data.create_client(request("A")).unwrap();
        assert!(matches!(
            data.create_client(request("A")),
            Err(ApiError::DuplicateDocument(_))
        ));
        // The rejected attempt must not have consumed an ID.
        assert_eq!(data.create_client(request("B")).unwrap(), 2);
    }

    #[test]
    fn credit_and_debit_update_balance() {
        let data = AppData::default();
        let id = data.create_client(request("A")).unwrap();
        assert_eq!(data.credit(id, dec("100.50")).unwrap(), dec("100.50"));
        assert_eq!(data.debit(id, dec("40.25")).unwrap(), dec("60.25"));
    }

    #[test]
    fn debit_beyond_balance_is_rejected() {
        let data = AppData::default();
        let id = data.create_client(request("A")).unwrap();
        data.credit(id, dec("10")).unwrap();
        assert!(matches!(
            data.debit(id, dec("10.01")),
            Err(ApiError::InsufficientFunds { .. })
        ));
        // Balance must be untouched after the rejection.
        assert_eq!(data.get_client(id).unwrap().balance, dec("10"));
    }

    #[test]
    fn unknown_client_is_not_found() {
        let data = AppData::default();
        assert!(matches!(
            data.credit(99, dec("1")),
            Err(ApiError::ClientNotFound(99))
        ));
        assert!(matches!(
            data.get_client(99),
            Err(ApiError::ClientNotFound(99))
        ));
    }

    #[test]
    fn drain_resets_balances_and_restore_adds_back() {
        let data = AppData::default();
        let a = data.create_client(request("A")).unwrap();
        let b = data.create_client(request("B")).unwrap();
        data.credit(a, dec("100")).unwrap();
        data.credit(b, dec("55.55")).unwrap();

        let snapshot = data.drain_balances().unwrap();
        assert_eq!(snapshot, vec![(a, dec("100")), (b, dec("55.55"))]);
        assert_eq!(data.get_client(a).unwrap().balance, Decimal::ZERO);

        // A credit lands between the failed write and the rollback…
        data.credit(a, dec("7")).unwrap();
        data.restore_balances(&snapshot).unwrap();
        // …and must survive the rollback.
        assert_eq!(data.get_client(a).unwrap().balance, dec("107"));
        assert_eq!(data.get_client(b).unwrap().balance, dec("55.55"));
    }

    #[test]
    fn file_sequence_starts_at_one() {
        let data = AppData::default();
        assert_eq!(data.next_file_sequence(), 1);
        assert_eq!(data.next_file_sequence(), 2);
    }
}
