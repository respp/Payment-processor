# Payment Processor API

*[English version](README.md)*

API REST de mini procesador de pagos desarrollada en Rust (Actix Web + Tokio). Los saldos se mantienen en memoria y pueden persistirse en disco bajo demanda.

## Cómo correr el servicio

```bash
cargo run       # levanta el servidor en http://127.0.0.1:8080
cargo test      # ejecuta los 13 tests: unitarios y HTTP en proceso sobre los handlers
```

## Endpoints

| Método | Path | Body / query | Éxito | Errores |
|--------|------|-------------|-------|---------|
| POST | `/new_client` | `{ client_name, birth_date, document_number, country }` | `201 { client_id }` | `400` input inválido, `409` documento duplicado |
| POST | `/new_credit_transaction` | `{ client_id, credit_amount }` | `200 { client_id, balance }` | `400` monto inválido, `404` cliente inexistente |
| POST | `/new_debit_transaction` | `{ client_id, debit_amount }` | `200 { client_id, balance }` | `400`, `404`, `422` fondos insuficientes |
| POST | `/store_balances` | — | `200 { file_name, clients_persisted }` | `500` error de escritura (saldos revertidos) |
| GET | `/client_balance?user_id={id}` | query param | `200` info del cliente + saldo | `404` cliente inexistente |

Todos los errores devuelven la misma estructura JSON: `{ "error": "<código>", "message": "<detalle>" }`, incluyendo JSON malformado y parámetros inválidos.

`/store_balances` escribe un archivo `DDMMYYYY_N.DAT` en el directorio de trabajo (`N` = contador por proceso, arranca en 1), con una línea `"{client_id} {balance}"` por cliente ordenado por ID, y luego resetea todos los saldos en memoria a cero.

## Postman

Importar `payment-processor.postman_collection.json`. La variable de colección `{{base_url}}` apunta por defecto a `http://127.0.0.1:8080`.

---

## Decisiones de diseño

### Layout del estado: contadores fuera del lock

```
AppData (Arc)
 ├── next_client_id: AtomicU32
 ├── file_sequence:  AtomicU32
 └── state:          RwLock<AppState>
```

Los IDs de cliente y el contador de archivos son `AtomicU32` que viven fuera del `RwLock`. Solo necesitan atomicidad — no hay ningún invariante que los ate al resto del estado y que requiera exclusión mutua con todo lo demás. Alojar un ID bajo un write guard serializaría innecesariamente las lecturas (ej: `GET /client_balance`) con la creación de clientes. `Ordering::Relaxed` es suficiente porque nos importa la unicidad, no el happens-before entre threads.

### `std::sync::RwLock` en lugar de `tokio::sync::RwLock`

Ningún write guard se mantiene a través de un punto `.await` — todas las secciones críticas son cortas, sincrónicas y solo CPU (lookups en hash map, aritmética, `mem::take`). En ese escenario, un lock bloqueante estándar es más barato que uno async: los locks async tienen overhead de bookkeeping y fuerzan un context switch incluso cuando no hay contención. El lint `await_holding_lock` de Clippy, que CI corre con `-D warnings`, marca cualquier intento de mantener uno de estos guards a través de un `.await`.

Los locks envenenados (si un thread panica mientras sostiene el lock) se convierten en HTTP 500 en lugar de propagar el panic a un request no relacionado.

### Toda la lógica de estado vive en `state.rs`, los handlers son delgados

Los handlers hacen tres cosas: extraer el input, llamar a un método de `AppData`, armar la respuesta. Nunca llaman a `lock()` o `write()` directamente. Esto permite auditar toda la disciplina de locking leyendo un solo archivo, y hace imposible introducir un deadlock o una validación omitida al agregar un nuevo endpoint.

### Los pares check-then-mutate comparten un único write guard

La verificación de unicidad de documento + insert, la verificación de fondos + débito, y el snapshot de saldos + reset corren bajo un único write guard adquirido una sola vez. Esto elimina la race condition clásica (TOCTOU) donde dos requests concurrentes pasan el check antes de que alguno haga commit de la mutación.

### `/store_balances`: drain dentro del lock, escritura fuera

Mantener un write lock durante toda la duración de una escritura a disco bloquearía todas las transacciones por potencialmente cientos de milisegundos. En cambio:

1. Se adquiere el write lock → se drenan todos los saldos a un snapshot y se resetean a cero → se libera el lock. Esto es rápido (solo memoria).
2. Se escribe el archivo *fuera* del lock. Las transacciones siguen fluyendo durante el I/O.
3. Si la escritura falla, se llama a `restore_balances`, que *suma* los montos del snapshot en lugar de sobreescribir. Esto preserva cualquier crédito o débito que haya llegado entre el drain y el write fallido — no se pierde dinero silenciosamente.

El caso borde: un débito que llega entre el drain y el rollback puede ver saldo cero y ser rechazado con 422. Es el modo de falla seguro — mejor rechazar temporalmente un débito válido que permitir un sobregiro o perder un crédito.

### Dinero: `rust_decimal`, no `f64`

La aritmética de punto flotante no puede representar exactamente muchas fracciones decimales. Usar `rust_decimal` (respaldado por un entero de 96 bits) garantiza que `0.10 + 0.20 == 0.30` siempre sea verdad. Los montos se validan para ser estrictamente positivos y con no más de 2 decimales (centavos enteros).

### Manejo de errores: `thiserror` + `ResponseError`

Un único enum `ApiError` cubre todos los fallos que el servicio puede producir. Cada variante mapea a un HTTP status code estable y un código de error legible por máquina. Ningún handler contiene un `match` sobre status codes — el mapeo está definido una sola vez en `errors.rs`. No hay llamadas a `.unwrap()` en paths de producción.

---

## Qué cambiaría en un sistema productivo

- **Persistencia**: reemplazar el `HashMap` en memoria por una base de datos (Postgres via `sqlx`). El diseño actual aísla todo el acceso al estado detrás de métodos de `AppData`, por lo que el swap estaría contenido en `state.rs`.
- **Autenticación**: el spec apunta a emisores de tarjetas como usuarios, por lo que mTLS o middleware de API key iría delante de cada endpoint.
- **Logging estructurado**: reemplazar `env_logger` con `tracing` + `tracing-subscriber` para output JSON con trace IDs por request.
- **Configuración**: leer host, puerto y directorio de salida de archivos desde variables de entorno (via `dotenvy`) en lugar de hardcodearlos.
- **Graceful shutdown**: drenar requests en vuelo antes de salir para evitar escrituras parciales en `SIGTERM`.
