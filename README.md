# Payment Processor API

[![CI](https://github.com/respp/Payment-processor/actions/workflows/ci.yml/badge.svg)](https://github.com/respp/Payment-processor/actions/workflows/ci.yml)

A small payment processor REST API in Rust (Actix Web + Tokio). Balances live in memory and can be persisted to disk on demand.

*[Versión en español](README.es.md)*

## Features

- Register clients, with document-number uniqueness enforced.
- Credit and debit transactions; overdrafts are rejected.
- Query a client's details and current balance.
- Persist all balances to a `.DAT` file and reset them to zero, with rollback if the write fails.
- Exact decimal money arithmetic (`rust_decimal`); amounts must be positive with at most 2 decimal places.
- One JSON error shape for every failure, including malformed JSON and bad query params.
- Concurrency-safe shared state: check-then-mutate operations run under a single write guard.

## Design decisions

### Check-then-mutate pairs share one write guard

Three operations check something and then mutate state based on it:

- document-number uniqueness check + client insert,
- funds check + debit,
- balance snapshot + reset.

Each pair runs under a single write guard, acquired once. This removes the classic TOCTOU race, where two concurrent requests both pass the check before either one commits its mutation (for example, two debits that each see enough funds and together overdraw the account).

### State layout: counters outside the lock

```
AppData (Arc)
 ├── next_client_id: AtomicU32
 ├── file_sequence:  AtomicU32
 └── state:          RwLock<AppState>
```

The client ID counter and the file sequence counter are `AtomicU32` values outside the `RwLock`. They only need atomicity: no invariant ties them to the rest of the state, so they do not need mutual exclusion with it. Allocating IDs under the write guard would needlessly serialize reads (e.g. `GET /client_balance`) with client creation. `Ordering::Relaxed` is enough because the code relies on uniqueness, not on cross-thread happens-before.

### `std::sync::RwLock`, not `tokio::sync::RwLock`

No guard is held across an `.await`. Every critical section is short, synchronous and CPU-only (hash map lookups, arithmetic, `mem::take`). In that case a standard blocking lock is cheaper than an async one: async locks add bookkeeping overhead even when there is no contention. Clippy's `await_holding_lock` lint, which CI runs with `-D warnings`, flags any attempt to hold one of these guards across an `.await`.

A poisoned lock (a thread panicked while holding it) becomes an HTTP 500 instead of propagating the panic into an unrelated request.

### All state transitions live in `state.rs`; handlers stay thin

Handlers do three things: extract input, call an `AppData` method, build the response. They never call `lock()` or `write()` directly. The whole locking discipline can be audited by reading one file, and a new endpoint cannot accidentally introduce a deadlock or skip a validation.

### `/store_balances`: drain inside the lock, write outside it

Holding the write lock during a file write would block every transaction for the length of the I/O. Instead:

1. Take the write lock, drain all balances into a snapshot and reset them to zero, release the lock. This is in-memory only, so it is fast.
2. Write the file *outside* the lock. Transactions keep flowing during I/O.
3. If the write fails, `restore_balances` *adds* the snapshot amounts back instead of overwriting them. Credits or debits that arrived between the drain and the failed write are preserved, so money is never silently lost.

Known gap: a debit that arrives between the drain and the rollback may see a zero balance and be rejected with 422. This is the safe failure mode: temporarily rejecting a valid debit is better than allowing an overdraft or losing a credit.

### Money: `rust_decimal`, not `f64`

Floating point cannot represent many decimal fractions exactly. `rust_decimal` (a 96-bit integer mantissa plus a decimal scale) guarantees that `0.10 + 0.20 == 0.30`. Amounts must be strictly positive with at most 2 decimal places (whole cents).

### Error handling: `thiserror` + `ResponseError`

A single `ApiError` enum covers every failure the service can produce. Each variant maps to a stable HTTP status code and a machine-readable error code. No handler matches on status codes: the mapping is defined once in `errors.rs`, in the `ResponseError` impl. There are no `.unwrap()` calls in production paths.

## Running

```bash
cargo run    # serves on http://127.0.0.1:8080
```

Host and port are currently hardcoded (see [Next steps](#what-would-change-in-production)).

## Testing

```bash
cargo test   # 13 tests: unit tests plus in-process HTTP tests of the handlers
```

CI runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` on every push and pull request.

A Postman collection is included for manual testing: import `payment-processor.postman_collection.json`. The `{{base_url}}` collection variable defaults to `http://127.0.0.1:8080`.

## API

| Method | Path | Body / query | Success | Errors |
|--------|------|--------------|---------|--------|
| POST | `/new_client` | `{ client_name, birth_date, document_number, country }` | `201 { client_id }` | `400` invalid input, `409` duplicate document |
| POST | `/new_credit_transaction` | `{ client_id, credit_amount }` | `200 { client_id, balance }` | `400` invalid amount, `404` unknown client |
| POST | `/new_debit_transaction` | `{ client_id, debit_amount }` | `200 { client_id, balance }` | `400`, `404`, `422` insufficient funds |
| POST | `/store_balances` | — | `200 { file_name, clients_persisted }` | `500` write failure (balances rolled back) |
| GET | `/client_balance?user_id={id}` | query param | `200` full client + balance | `404` unknown client |

Every error has the same JSON shape:

```json
{ "error": "<machine_code>", "message": "<human_detail>" }
```

This includes malformed JSON and bad query params, which custom `JsonConfig` / `QueryConfig` error handlers in `main.rs` route through `ApiError`.

`/store_balances` writes `DDMMYYYY_N.DAT` to the working directory (`N` is a per-process counter starting at 1), one `"{client_id} {balance}"` line per client sorted by ID, then resets all in-memory balances to zero.

## What would change in production

- **Persistence**: replace the in-memory `HashMap` with a database (Postgres via `sqlx`). All state access is behind `AppData` methods, so the change would stay inside `state.rs`.
- **Authentication**: the intended users are payment issuers, so mTLS or API-key middleware would sit in front of every endpoint.
- **Structured logging**: replace `env_logger` with `tracing` + `tracing-subscriber` for JSON logs with per-request trace IDs.
- **Configuration**: read host, port and output directory from environment variables (via `dotenvy`) instead of hardcoding them.
- **Graceful shutdown**: drain in-flight requests before exiting, to avoid partial writes on `SIGTERM`.
