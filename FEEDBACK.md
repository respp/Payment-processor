# Feedback del proceso de entrevista — Rust Senior Developer

Este documento recopila los 7 puntos de feedback recibidos tras el rechazo, con mi
evaluación de cada uno y el estado real en el que quedó el proyecto. Los cambios de
código que sí se aplicaron están marcados en el diff con comentarios `// ! FEEDBACK N:
...` (usan el prefijo `!` para que resalten con extensiones tipo "Better Comments"),
así que se pueden buscar en GitHub con `FEEDBACK` para ir directo a cada uno.

---

## 1. Organización del proyecto (dirtree)

> La estructura de carpetas es muy plana: todos los handlers están en un único
> archivo, todos los modelos en otro, etc. Te recomendamos modularizar por dominio o
> feature (por ejemplo, `handlers/clients.rs`, `handlers/documents.rs`,
> `models/clients.rs`, etc.). Esto mejora mucho la navegabilidad del código a medida
> que el proyecto crece.

**Mi feedback:** de acuerdo sin objeciones. Un `handlers.rs`/`models.rs` planos
escalan mal apenas se agregan un par de features más — cada PR toca el mismo archivo
gigante y el diff se vuelve difícil de leer. Modularizar por feature es el estándar
en proyectos Rust de este tamaño.

**¿Dónde mejoró?** Sí, se aplicó completo:
- `src/handlers.rs` → `src/handlers/{mod,clients,transactions,balances}.rs`
- `src/models.rs` → `src/models/{mod,client,transaction,balance}.rs`

Cada submódulo agrupa un dominio (clientes, transacciones, balances) y el `mod.rs`
de cada carpeta re-exporta lo público, así que ningún import externo tuvo que
cambiar. `main.rs`, `state.rs` y `errors.rs` se dejaron como archivos únicos a
propósito: no tienen una división natural por feature y `state.rs` en particular
necesita quedarse en un solo lugar para poder auditar el locking de una sola pasada.

---

## 2. Documentación

> La documentación general del proyecto se percibe generada mayormente con IA, sin
> mucha revisión o personalización. Los rustdocs también quedaron pobres: muy pocas
> funciones están documentadas. Vale la pena invertir tiempo acá, especialmente en
> funciones públicas o con lógica no trivial.

**Mi feedback:** el señalamiento sobre el README (tono genérico, traducción 1:1
ES/EN, bullets repetidos) es válido. El de rustdocs también: varias funciones
públicas y DTOs no tenían ni una línea de `///`.

**¿Dónde mejoró?** **No se resolvió.** Decidiste no continuar con la reescritura del
README ("lo del readme no lo haré al final"), y como consecuencia tampoco se retomó
la tarea pendiente de agregar rustdocs a los handlers (`new_client`,
`client_balance`, `new_credit_transaction`, `new_debit_transaction`) ni a varios DTOs
(`NewClientRequest`, `ClientBalanceQuery`, `NewClientResponse`, `CreditRequest`,
`DebitRequest`, `BalanceResponse`). Quedó como deuda pendiente, no hay comentario `!`
en código para este punto porque no se tocó nada.

---

## 3. `main.rs` y `app_data`

> No queda claro por qué se clona `app_data` en el main si no se necesita el
> ownership después de ese punto. Vale la pena repasar cuándo un `clone()` es
> realmente necesario versus cuándo alcanza con pasar una referencia o mover el
> valor directamente.

**Mi feedback:** el comentario original del reviewer da a entender que el `.clone()`
sobra, pero no es así — es necesario y no hay forma de evitarlo con una referencia.
`HttpServer::new` recibe una *factory closure* que Actix llama una vez por cada
worker thread, y esa closure tiene que ser `Fn` (no `FnOnce`): si el `data` se
moviera directo adentro sin clonar, la primera llamada lo consumiría y ya no
compilaría para más de un worker. Como `web::Data<T>` es un `Arc<T>` por dentro, el
`.clone()` es sólo un incremento de refcount, no una copia del `HashMap` de
clientes — cada worker termina con un handle al mismo estado.

**¿Dónde mejoró?** El código ya era correcto; lo único que faltaba era explicarlo.
Se agregó un comentario en `src/main.rs` justo donde ocurre el `.clone()`
explicando el motivo (factory `Fn`, `Arc` por debajo). No hubo cambio de lógica.

---

## 4. Manejo de errores (`ErrorBody`)

> El manejo de errores está bien resuelto en general, salvo por un detalle:
> `ErrorBody` es innecesario, ya que el propio handler de error de Actix arma esos
> campos (`error` y `message`) por vos.

**Mi feedback:** este punto es incorrecto. Fui a verificar directamente el código
fuente de `actix-web` 4.14.0 instalado
(`~/.cargo/registry/.../actix-web-4.14.0/src/error/response_error.rs`): la
implementación por defecto de `ResponseError::error_response()` devuelve **texto
plano**, no JSON, y no arma ningún campo `error`/`message`. Si se sacara
`ErrorBody`, las respuestas de error dejarían de ser JSON estructurado y romperían
cualquier cliente que hoy parsea `{"error": ..., "message": ...}`.

**¿Dónde mejoró?** **No se aplicó ningún cambio** — la premisa del feedback es
factualmente errónea, verificada contra el source de la librería. Vale la pena
transmitirle esto al recruiter con la referencia al archivo fuente.

---

## 5. Validación

> Este es el bloque con más para trabajar:
> - La función de validación corta en el primer error encontrado.
> - Para evitar `ApiError::Validation` repetido dentro de un `for`, que la función
>   devuelva un `Vec<String>` y sea el caller quien lo envuelva en el error.
> - `Result<Self, ApiError>` no hace falta; conviene un tipo propio para
>   desacoplar la validación del tipo de error de la API.
> - No hace falta ownership mutable para validar; con `&self` alcanza.
> - Falta normalización de criterio entre validaciones (tipos, nombres,
>   ubicación distintos entre sí).

**Mi feedback:** de acuerdo en los cinco sub-puntos, sin excepción. Este era el
gap real más grande del proyecto.

**¿Dónde mejoró?** Se reescribió todo el esquema de validación:
- Nuevo trait `Validate` en `src/validation.rs`, deliberadamente sin importar
  `errors` — desacopla "qué está mal" de "cómo se traduce a HTTP".
  `fn validate(&self) -> Vec<String>` toma `&self` (no ownership) y **nunca
  corta en el primer error**: junta todos los problemas encontrados.
- `ApiError::Validation` pasó de `String` a `Vec<String>` (`src/errors.rs`,
  `src/main.rs`).
- `NewClientRequest`, `CreditRequest` y `DebitRequest` implementan el mismo
  trait con el mismo criterio (antes cada uno tenía su propia forma).
- Se separó validar (`&self`, sin mutar) de normalizar (`normalized(self) ->
  Self`, consume y trimea) — antes estaban mezclados.
- `validate_amount` ahora reporta positividad *y* precisión de centavos como
  errores independientes en la misma pasada, en vez de uno u otro.
- Los handlers (`handlers/clients.rs`, `handlers/transactions.rs`) son los que
  arman `ApiError::Validation(errors)` — `validate()` ya no conoce `ApiError`.
- Se agregaron tests unitarios nuevos por cada DTO y un test de integración
  HTTP (`validation_reports_every_invalid_field_in_one_response`) que prueba
  que 3 campos inválidos a la vez devuelven los 3 errores en una sola
  respuesta 400, no uno por request.

---

## 6. Diseño del estado compartido (`AppState`)

> - El nombre `AppState` es demasiado genérico.
> - `clients` y `documents` comparten un `RwLock`: escribir en uno bloquea al
>   otro innecesariamente, volviendo síncrono código async.
> - `documents` sólo lo modifica `create_client` al recibir un
>   `NewClientRequest`, el resto sólo lee — ahí alcanzaría con un `Arc<>` sin
>   lock de escritura compartido.
> - Se nota mentalidad más cercana a C que a Rust/Tokio.
> - El `AtomicU32` está bien elegido, pero no hace falta que viva junto al
>   resto del estado.

**Mi feedback:** mixto, punto por punto:
- Nombre genérico: correcto, se corrigió.
- `AtomicU32` fuera del lock: **ya estaba así** antes del feedback — los
  contadores (`next_client_id`, `file_sequence`) ya vivían como campos
  `AtomicU32` propios en `AppData`, no dentro del `RwLock`.
- Separar `clients` y `documents` en locks distintos: **la premisa es
  incorrecta.** Verifiqué con `grep -rn "\.documents" src/` que `documents` se
  lee y escribe en un único lugar de todo el codebase: `create_client`, y ahí
  se tocan *ambos* campos bajo el mismo guard porque tienen que ser atómicos
  entre sí — si no, dos registros concurrentes con el mismo `document_number`
  podrían pasar la verificación de unicidad antes de que ninguno de los dos
  confirme el insert (una race TOCTOU). Separar los locks no elimina
  contención real (nada más toca `documents`) y obligaría a mantener dos locks
  ordenados en el único lugar donde se usan juntos — peor, no mejor.

**¿Dónde mejoró?** Parcial:
- `AppState` → `ClientRegistry` en `src/state.rs` (nombre + doc explicando por
  qué comparte un solo lock, citando que `documents` tiene un único call
  site).
- El split de locks **no se aplicó** — se documentó en código por qué no
  corresponde, en vez de aplicar un cambio que hubiese sido un paso atrás.

---

## 7. `std::sync::RwLock` en vez de `tokio::sync::RwLock`

> Hallazgo más importante de la revisión: el proyecto usa el `RwLock` de
> `std::sync` en un contexto async en vez del de `tokio::sync`. Es un error
> grave porque el lock de `std` bloquea el hilo de forma síncrona en lugar de
> cederle el control al executor de tokio, lo cual puede degenerar en
> bloqueos del runtime bajo carga. Punto no negociable para código async en
> producción.

**Mi feedback:** este punto está **fácticamente al revés**. Fui a leer
directamente el source de `tokio` 1.53.1 instalado
(`~/.cargo/registry/.../tokio-1.53.1/src/sync/mutex.rs`), donde la
documentación oficial dice lo contrario de lo que afirma el feedback: para
datos en memoria con secciones críticas cortas, sólo CPU y que **nunca cruzan
un `.await`**, se prefiere el lock bloqueante de `std::sync` — el lock async
de `tokio::sync` está pensado para cuando hay que *mantener* el guard
mientras se espera IO (cruzando un `.await`), no para proteger un `HashMap`
en memoria. La regla real no es "std vs tokio", es "nunca sostener un guard
across `.await`" — y el código ya la cumple: ningún método de `AppData` es
`async fn`, y `store_balances` (el único handler que hace IO) saca el balance
del lock *antes* de escribir a disco, así que el guard nunca convive con una
espera async.

**¿Dónde mejoró?** **No se aplicó ningún cambio** — cambiar a
`tokio::sync::RwLock` acá sería peor, no mejor: agrega overhead de scheduling
sin necesidad real, justo lo contrario de lo que dice la documentación oficial
de tokio. Se agregó un comentario en `src/state.rs` (marcado `FEEDBACK 7`)
documentando por qué `std::sync::RwLock` es la elección correcta, para que
quede explícito y no como un descuido. Vale la pena citarle al recruiter la
doc de tokio si este punto vuelve a salir.

---

## Resumen

| # | Punto | Estado |
|---|-------|--------|
| 1 | Organización del proyecto | ✅ Aplicado completo |
| 2 | Documentación | ⏸️ No resuelto (decisión de no continuar) |
| 3 | `main.rs` / `app_data` clone | ✅ Aclarado (ya era correcto) |
| 4 | `ErrorBody` innecesario | ❌ Rechazado (premisa incorrecta, verificado en source) |
| 5 | Validación | ✅ Aplicado completo |
| 6 | Diseño de `AppState` | ✅ Parcial (nombre sí, split de locks no) |
| 7 | `std::sync` vs `tokio::sync` `RwLock` | ❌ Rechazado (premisa incorrecta, verificado en source) |
