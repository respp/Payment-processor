# Payment Processor API

Mini payment processor REST API built in Rust (Actix Web + Tokio). Balances are kept in memory and can be persisted to disk on demand.

## Run

```bash
cargo run       # serves on http://127.0.0.1:8080
cargo test      # unit + integration tests (13 tests)
```

## Endpoints

| Method | Path | Body / query | Success | Errors |
|--------|------|-------------|---------|--------|
| POST | `/new_client` | `{ client_name, birth_date, document_number, country }` | `201 { client_id }` | `400` invalid input, `409` duplicate document |
| POST | `/new_credit_transaction` | `{ client_id, credit_amount }` | `200 { client_id, balance }` | `400` bad amount, `404` unknown client |
| POST | `/new_debit_transaction` | `{ client_id, debit_amount }` | `200 { client_id, balance }` | `400`, `404`, `422` insufficient funds |
| POST | `/store_balances` | — | `200 { file_name, clients_persisted }` | `500` write failure (balances rolled back) |
| GET | `/client_balance?user_id={id}` | query param | `200` full client + balance | `404` unknown client |

All errors return the same JSON shape: `{ "error": "<machine_code>", "message": "<human_detail>" }` — including malformed JSON and bad query params, which are intercepted by custom `JsonConfig`/`QueryConfig` extractors in `main.rs`.

`/store_balances` writes `DDMMYYYY_N.DAT` to the working directory (`N` = per-process counter starting at 1), one `"{client_id} {balance}"` line per client sorted by ID, then resets all in-memory balances to zero.

## Postman

Import `payment-processor.postman_collection.json`. The `{{base_url}}` collection variable defaults to `http://127.0.0.1:8080`.

---

## Design decisions

### State layout: counters outside the lock

```
AppData (Arc)
 ├── next_client_id: AtomicU32
 ├── file_sequence:  AtomicU32
 └── state:          RwLock<AppState>
```

Client IDs and the file sequence counter are `AtomicU32` values that live outside the `RwLock`. They only need atomicity — there is no invariant tying them to the rest of the state that would require mutual exclusion with everything else. Allocating an ID under a write guard would unnecessarily serialize reads (e.g. `GET /client_balance`) with client creation. `Ordering::Relaxed` is sufficient because we rely on uniqueness, not on cross-thread happens-before.

### `std::sync::RwLock` instead of `tokio::sync::RwLock`

No write guard is ever held across an `.await` point — all critical sections are short, synchronous, and CPU-only (hash map lookups, arithmetic, `mem::take`). In that scenario a standard blocking lock is cheaper than an async lock: async locks carry bookkeeping overhead and force a context switch even when contention is zero. Using `std::sync` also makes the locking discipline obvious: the compiler will reject any attempt to hold a guard across `.await` at compile time.

Poisoned locks (which occur if a thread panics while holding the lock) surface as HTTP 500 instead of propagating the panic to an unrelated request.

### All state transitions live in `state.rs`, handlers stay thin

Handlers do three things: extract input, call a method on `AppData`, shape a response. They never call `lock()` or `write()` directly. This means the entire locking discipline can be audited by reading one file, and adding a new endpoint cannot accidentally introduce a deadlock or a missed validation.

### Check-then-mutate pairs share one write guard

Document-number uniqueness check + insert, funds check + debit, and balance snapshot + reset each run under a single write guard acquired once. This eliminates the classic TOCTOU race where two concurrent requests both pass a check before either commits the mutation.

### `/store_balances`: drain inside the lock, write outside it

Holding a write lock for the entire duration of a file write would block all transactions for potentially hundreds of milliseconds. Instead:

1. Acquire write lock → drain all balances into a snapshot, reset to zero → release lock. This is fast (in-memory only).
2. Write the file *outside* the lock. Transactions keep flowing during I/O.
3. If the write fails, call `restore_balances`, which *adds* the snapshot amounts back rather than overwriting. This preserves any credits or debits that arrived between the drain and the failed write — money is never silently lost.

The gap: a debit that arrives between drain and rollback may see a zero balance and be rejected with 422. This is the safe failure mode — it is better to temporarily reject a valid debit than to allow an overdraft or lose a credit.

### Money: `rust_decimal`, not `f64`

Floating-point arithmetic cannot represent many decimal fractions exactly. Using `rust_decimal` (backed by a 96-bit integer mantissa and a decimal exponent) means that `0.10 + 0.20 == 0.30` is always true. Amounts are validated to be strictly positive and carry at most 2 decimal places (whole cents).

### Error handling: `thiserror` + `ResponseError`

A single `ApiError` enum covers every failure the service can produce. Each variant maps to a stable HTTP status code and a machine-readable error code string. No handler contains a `match` on status codes — the mapping is defined once in `errors.rs` and enforced by the `ResponseError` impl. There are no `.unwrap()` calls in production paths.

---

## What would change in a production system

- **Persistence**: replace the in-memory `HashMap` with a database (Postgres via `sqlx`). The current design isolates all state access behind `AppData` methods, so the swap would be contained to `state.rs`.
- **Authentication**: the spec targets payment issuers as users, so mTLS or API key middleware would sit in front of every endpoint.
- **Structured logging**: replace `env_logger` with `tracing` + `tracing-subscriber` for JSON log output with per-request trace IDs.
- **Configuration**: read host, port, and file output directory from environment variables (via `dotenvy`) rather than hardcoding them.
- **Graceful shutdown**: drain in-flight requests before exiting to avoid partial writes on `SIGTERM`.
