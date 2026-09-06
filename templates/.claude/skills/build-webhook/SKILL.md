---
name: build-webhook
description: Implements a slice whose trigger is an inbound external event — a webhook — in Rust with skilj, from a slice.json
---

# Build a webhook slice

> Before anything else, read the definition at
> `.build-kit/.slices/{Context}/{slice}/slice.json`. Never invent fields that
> aren't there.

> And read `.build-kit/CLAUDE.md`. The tag rule and the `pii` mapping apply
> here exactly as in any other write slice.

---

## When it's this shape and not an automation

The difference isn't "there's an external system": it's **who starts**.

| | who starts | signal in `slice.json` |
|---|---|---|
| **Automation** | us, polling a TODO queue | `processors[]` non-empty |
| **Webhook** | the external system, whenever it likes | an `events[]` element with **`context: "EXTERNAL"`** |

`Element.context` is a per-element field on the schema this kit was built
against (not the slice-level `context`, which names the bounded context) —
it's a better signal than parsing `description` prose for who-starts
language, though the prose still carries the rest of this slice's own rules
and is worth reading in full.

---

## Step 0 — The decision `slice.json` doesn't make for you: does this need a domain rejection?

**This is genuinely different from other Build-Kits' webhook pattern, and it's
the first thing to settle — it decides which skilj mechanism the rest of this
skill uses.** skilj's `EventType` has no `decide()`-equivalent hook: an event
created via `external_creation_allowed`/`POST /v1/events/external` is accepted
unconditionally once its token, signature, and schema check out — there is no
business-rule rejection path the way a `CommandType` has. So:

- **If `specifications[]` includes a `SPEC_ERROR` scenario for this slice**
  (the board expects some inbound bodies to be rejected for a domain reason,
  not just a bad signature or malformed JSON) → **model this as a
  `CommandType`, triggered by the external system instead of a normal
  caller.** Build it with `/build-state-change` first — real
  `decide()`-based `Accepted`/`Rejected` semantics, tag-scoped consistency,
  the works — then come back here only for the web-layer wiring (Steps 3-5
  below use `skilj_core::db::decide_and_submit_command` in-process, **not**
  `ExternalEventToken`/`create_and_insert_external_event` at all).
- **If the slice is a pure "record what arrived" fact with no domain gate at
  all** (provenance/forensics, a mirrored upstream stream) → model it as an
  `EventType` with `external_creation_allowed() -> true`. Steps 3-5 below use
  `ExternalEventToken`/`create_and_insert_external_event` instead.

**If you can't tell which from `slice.json`, invoke `request-feedback`** — the
two paths produce genuinely different code and a different failure mode if
the provider ever sends something the domain should refuse.

Both paths share Steps 1-2 and the translation/signature rules below;
Steps 3-5 fork on which path you're on.

---

## Step 1 — The domain shape

Build the command+event (CommandType path) or just the event (EventType
path) with `/build-state-change`'s own Steps 2/4/5/6 — field translation,
tags from `idAttribute`, `pii` → `sensitive_fields()`, all apply unchanged.
This skill only covers what that one doesn't: the web layer in front of it.

---

## Step 2 — The translation, and which way it points

**Our domain fact leads and the webhook body fills it in**, not the other way
round:

- **One event.** Don't emit a `WebhookReceived` alongside the business fact —
  "something arrived" isn't a domain fact, and giving it its own type drags
  the foreign shape onto the timeline.
- **The raw body travels as a technical attribute** (`rawBody`,
  `technicalAttribute: true` in `slice.json`) — a plain payload field, for
  forensics and re-derivation. It is never a tag, never `pii`, and never
  consumed by any `Projection`.
- **Field names are domain names**, not the provider's.
- **The translation is a pure function**, separate from the axum handler:
  `fn to_domain(body: &[u8]) -> Result<<Command|Event>Payload, TranslationError>`.
  Pull it out so it's unit-testable without an HTTP request — the only part
  of a webhook that really deserves one.

---

## Step 3 — The route and the signature check

**File:** `src/<context_snake_case>_web.rs` (or alongside the context module
if the crate is small) — **not** merged into skilj's own `/v1/*`/`/graphql`
routers; a sibling `axum::Router` merged into `app` in `src/bin/server.rs`
next to `rest`/`graphql`.

Unlike a framework where the request body is already consumed by the time a
handler sees it, **axum makes this simple**: extract the raw body as
`axum::body::Bytes` instead of `Json<T>`, verify the signature against those
exact bytes, then deserialize — no separate "cache the raw body first" plug
needed.

```rust
async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let Some(signature) = headers.get("x-<external>-signature").and_then(|v| v.to_str().ok()) else {
        return StatusCode::UNAUTHORIZED;
    };
    if !valid_signature(signature, &body, &state.webhook_secret) {
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(payload) = to_domain(&body) else {
        // Malformed body the domain can't even parse - still 200 (see below),
        // never a 4xx that would make the provider retry the same bad body.
        return StatusCode::OK;
    };
    // Step 4/5's submission goes here.
    StatusCode::OK
}

/// `Plug.Crypto.secure_compare/2`'s Rust equivalent - comparing signatures
/// with `==` leaks information through timing. `subtle::ConstantTimeEq` or
/// a `ring`/`hmac` crate's own constant-time compare, never `==`.
fn valid_signature(signature: &str, body: &Bytes, secret: &str) -> bool { /* ... */ }
```

### Which status code to return

- **200 (or 201, matching whatever this provider expects as "received") even
  when the domain rejects.** The provider only knows whether it arrived, not
  whether it was useful — a 4xx/5xx sets it retrying a body already known to
  be no good.
- **401 only when the signature fails.** That's genuinely the caller's
  problem.
- **Answer fast.** If the work is long, record the fact and leave the rest to
  an automation (`/build-automation`) — the webhook only has to accept it.

**If `slice.json` says nothing about a signature, ask** — invoke
`request-feedback` before leaving the route open. An unverified webhook is a
public endpoint that writes to the event store.

---

## Step 4a — Submission: the CommandType path

```rust
let command_type = db::get_command_type(&state.pool, BOUNDED_CONTEXT, "<Command>").await?;
let payload_json = serde_json::to_string(&payload)?;
let outcome = db::decide_and_submit_command(
    &state.pool, state.dispatcher.as_ref(), state.projection_dispatcher.as_ref(),
    state.snapshot_dispatcher.as_ref(), &state.event_broadcaster, &state.event_cache,
    &command_type, &payload_json,
    &format!("webhook:<external>"),   // client_id - identifies the source, not a real CommandToken
    state.encryption_master_key.as_ref(), Utc::now(),
    provider_delivery_id.as_deref(),  // idempotency_key - the provider's own delivery/event id,
                                       // if it sends one; skilj's own idempotency-key mechanism
                                       // (the same one POST /v1/commands/trigger uses) then makes
                                       // a retried delivery return the prior outcome instead of
                                       // re-deciding - use this whenever the provider's retries are
                                       // otherwise indistinguishable from a genuinely new request.
).await?;
```

Map `SubmitCommandOutcome::Rejected` to **still 200** (Step 3's own rule) —
never surface the rejection reason back to the provider; log it instead.

---

## Step 4b — Submission: the EventType path

```rust
let token = db::get_external_event_token(&state.pool, &state.webhook_token_id)
    .await?
    .expect("minted once at boot, see server.rs");
let outcome = db::create_and_insert_external_event(
    &state.pool, state.projection_dispatcher.as_ref(), &state.event_broadcaster, &state.event_cache,
    &token, serde_json::to_string(&payload)?,
    String::from_utf8_lossy(&body).into_owned(),  // sourceContent - the raw body, for forensics
    None,                                           // sourceContext - optional, name the specific
                                                     // provider/integration if more than one feeds
                                                     // this same EventType
    dedupe,   // see below
    Utc::now(), state.encryption_master_key.as_ref(),
).await?;
```

**`dedupe: Option<DedupeCursor{partition_key, sequence}>`** — use only when
the inbound source is itself partitioned/ordered (a Kafka-sourced relay, a
provider with its own strictly-increasing per-stream sequence number) — never
invent a `partition_key`/`sequence` pair that isn't genuinely how the source
already orders itself. **If the provider can redeliver the same webhook and
the source is *not* partitioned/ordered, this path has no built-in
redelivery protection at all** — prefer Step 4a instead (its idempotency-key
mechanism covers this case) rather than accepting silent duplicates.

The `token` is loaded, not authenticated over HTTP — mint it once (alongside
the `CommandToken`s already minted in `server.rs`) and remember its `id`;
there's no secret-comparison step needed for an in-process call, only the
signature check in Step 3 gates this route at all.

---

## Step 5 — Tests

**File:** `tests/<context_snake_case>.rs`

Two, neither touching the network:

- **The pure `decide()`/`to_domain` tests** from `/build-state-change`
  (CommandType path) or a plain unit test of `to_domain` alone (EventType
  path) — the board's specifications against the translation, no HTTP.
- **A handler-level test** (axum's own `tower::ServiceExt::oneshot`, no real
  network) pinning down what's specific to a webhook:
  - A valid body **with a valid signature** → 200 (or the provider's expected
    code) and the event/command was actually written.
  - A valid body **with an invalid signature** → **401** and **nothing
    written**. The test that actually matters.
  - A body the domain rejects (CommandType path) or fails to parse (either
    path) → still **200**, and nothing written.
  - **The same body twice** → one event, or (CommandType path with an
    idempotency key) the same outcome replayed, never a duplicate fact.

---

## Step 6 — Quality gate

```
cargo build && cargo test
cargo test <context_snake_case>::
```

---

## Final check against `slice.json`

- [ ] Step 0's decision was made deliberately (CommandType vs. EventType
      path), not defaulted without checking `specifications[]`.
- [ ] **One domain event**, not a "webhook received" one.
- [ ] Field names are domain names, not the provider's.
- [ ] The raw body is a `technicalAttribute` field, never a tag, never `pii`.
- [ ] The signature is verified against the **raw bytes** (`axum::body::Bytes`),
      with a constant-time comparison.
- [ ] The webhook route lives outside skilj's own `/v1/*`/`/graphql` routers.
- [ ] The secret comes from configuration/environment, never hardcoded.
- [ ] There's a test for invalid signature → 401 and no write.
- [ ] There's a test for a retry → no duplicate fact (idempotency key, or
      `DedupeCursor` only if the source is genuinely partitioned/ordered).
