---
name: build-automation
description: Implements an automation slice (a background worker that watches a TODO queue, calls an external system, and records the result as our own fact) in Rust with skilj, from a slice.json
---

# Build an automation slice

> Before anything else, read the definition at
> `.build-kit/.slices/{Context}/{slice}/slice.json`. Never invent fields that
> aren't there.

> And read `.build-kit/CLAUDE.md`. This slice shape deviates most from what
> the board appears to say, so its tag rule and `pii` mapping matter double
> here.

---

## What an automation slice is

Nobody clicks anything. A background worker watches a **TODO queue** — a
`Projection`, not a table — does the work, and **writes a fact of our own**.

```
TODO queue (a Projection, per /build-state-view)
  → a tokio task, spawned once in src/bin/server.rs, polling on an interval
      → external call
      → skilj_core::db::decide_and_submit_command, in-process → event
```

Build the TODO queue's read model with `/build-state-view` and the command it
ends up submitting with `/build-state-change` **first** — this skill only
covers the worker loop connecting them.

**Not a separate binary, not a `GenServer`.** This is one `async fn`, spawned
via `tokio::spawn` inside the same process that runs the axum server — see
`src/bin/server.rs`'s own doc comment for exactly where. Unlike a framework
where a single-process mailbox forces the external call into its own isolated
task to avoid blocking every other message, **tokio already runs this task
independently of the axum server and of every other automation's own task** —
an `.await`ed `reqwest` call inside the worker's own loop yields to the
scheduler, it doesn't block anything else. There's no `Task.Supervisor.async_nolink`-equivalent
nesting to add for that reason alone; only add a nested `tokio::spawn` per
item when you actually want more than one external call in flight
concurrently (see the ceiling below).

---

## Step 1 — The TODO queue exists and you don't build it here

`processors[].dependencies` points at a `READMODEL` that is the queue — it's
usually a separate slice on the board, with its own `slice.json`.

**If that slice isn't built, stop.** Build it first with `/build-state-view`,
or invoke `request-feedback` if it isn't on the board at all.

A TODO queue is "what was asked minus what was resolved" — it folds the event
that opens the work and the one that closes it, leaving out what's already
closed. **It's a fold, not a subtraction** — don't keep a separate counter.
Per `/build-state-view`'s own note: this is the one case that stays **unkeyed**
(`Projection::keys()` at its default) — the consumer is this worker, which has
no single entity to scope by.

---

## Step 2 — The anti-corruption layer, and which way it points

When the slice calls an external system, the temptation is to model "we
received this." **Don't.**

> Our domain fact leads and the external response fills it in, not the other
> way round.

If the event is "the external response minus some fields," a change in the
external system propagates through the whole timeline. If the event is our
own fact and the external system is an input, a change there only moves the
mapping, in one place.

- **One event, not two.** Don't emit an `XResponseReceived` alongside the
  domain fact.
- **The raw response travels as a technical attribute**
  (`rawResponse`, `technicalAttribute: true`) for forensics/re-derivation. It
  is never a tag, never `pii`, never projected into any read model.
- **Field names are domain names**, not the external API's — never
  `risk_pscore`: `verdict`. Which layer measured it goes in a provenance
  field, never in the name.
- **The event name's preposition matters.** `…EvaluatedWithX` says X computed
  and we judged; `…EvaluatedByX` would say X made our judgement. Match
  whichever the board's own title already says — don't improve on it.

### Where the source goes: the name, or a field

| who judges | where it goes |
|---|---|
| a third party evaluates and returns a verdict | the **event name** |
| we compute it ourselves against a file/artefact | a **provenance field** (self-describing, e.g. `"Provita ANP 2023-07-29"`, not a bare date) |

In the second case there's **no `source` field** — it would imply an external
evaluator where there is none.

#### If you compute against a deployed file, the file is an interface too

A data file read at runtime is as much an external system as an HTTP API — it
has a shape you don't control. **Never hand-write the fixture for a reader of
a deployed artefact.** Derive it from the real file and add a test comparing
the two:

```rust
#[test]
fn the_fixture_keys_match_the_real_files_keys() {
    let Ok(contents) = std::fs::read_to_string(deployed_path()) else {
        return; // absent is not a failure - large artefacts often live outside the repo
    };
    let real: HashSet<String> = /* parse contents, collect keys */;
    assert_eq!(FIXTURE_KEYS.iter().copied().collect::<HashSet<_>>(), real,
        "the deployed file changed vocabulary");
}
```

A hand-written fixture asserts the reader against *itself* — both sides share
the author's belief about the file's shape, so the test proves nothing about
the file. If the belief is wrong, every test stays green while the real read
silently returns empty/default values — and if the command validates
required fields (it should), the write is then rejected forever and the item
never leaves the queue, with no error anywhere obvious.

### Translation happens inline

The board may show several slices for one external call (request → external
event → response view → translator → internal command → internal event). **In
code it's one worker that calls and submits.** The board prioritises making
the system boundary visible; the implementation prioritises not having
handlers that do nothing.

If the external call is genuinely asynchronous instead — an inbound webhook,
not something we poll for — the entry point is an axum route, not a worker.
Use `/build-webhook`.

---

## Step 3 — The worker loop

**File:** `src/<context_snake_case>.rs`, appended (or a `processor.rs` module
alongside it if the loop body is long).

```rust
use futures::stream::{FuturesUnordered, StreamExt};   // add `futures` to Cargo.toml

pub async fn run_<slice>(pool: Pool, dispatcher: Arc<dyn CommandDispatcher>, /* ... */) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    loop {
        interval.tick().await;
        if let Err(e) = process_pending(&pool, dispatcher.as_ref() /* ... */).await {
            tracing::warn!("poll failed: {e}");
        }
    }
}

async fn process_pending(pool: &Pool, /* ... */) -> Result<(), Error> {
    let raw = db::get_projection_state(pool, BOUNDED_CONTEXT, "<Queue>", "").await?;
    let queue: <Queue>State = raw.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or_default();
    let mut pending = queue.pending.into_iter();
    let mut in_flight = FuturesUnordered::new();

    // Fill up to the ceiling - `room_for` is the same pure function Step 5
    // tests directly; this is the one place it actually gates anything.
    while room_for(in_flight.len(), IN_FLIGHT) {
        let Some(item) = pending.next() else { break };
        in_flight.push(process_one(pool, item));
    }
    while let Some(outcome) = in_flight.next().await {
        // On success, refill the slot that just freed immediately; on
        // failure, leave it idle until the next poll tick - an instant
        // retry would turn a rejected call into a tight loop against
        // someone else's API; let the poll interval be the backoff. Any
        // item never reached this poll stays exactly where it already
        // lives, the durable Projection queue, so a restart loses nothing.
        if outcome.is_ok() {
            if let Some(item) = pending.next() {
                in_flight.push(process_one(pool, item));
            }
        }
    }
    Ok(())
}

/// `Err(())` only signals "don't chain another item onto this freed slot
/// yet" to the loop above - every branch below logs its own failure via
/// `tracing` before returning it, so nothing is silently discarded the way
/// a bare `let _ = ...` or `.unwrap()` would.
async fn process_one(pool: &Pool, item: QueueItem) -> Result<(), ()> {
    let response = match call_external(&item).await {
        Ok(response) => response,
        Err(e) => {
            tracing::error!("<external> failed for {item:?}: {e}");
            return Err(());
        }
    };
    let payload = to_domain(response, &item);   // pure - see Step 2

    let command_type = match db::get_command_type(pool, BOUNDED_CONTEXT, "<Command>").await {
        Ok(Some(ct)) => ct,
        Ok(None) => {
            tracing::error!("<Command> isn't registered - is the slice that defines it deployed?");
            return Err(());
        }
        Err(e) => {
            // A transient DB blip here must never panic this task - unlike
            // an `.unwrap()`, this just leaves the item in the queue for
            // the next poll to retry, exactly like any other failure below.
            tracing::warn!("could not load <Command>'s CommandType, will retry next poll: {e}");
            return Err(());
        }
    };

    let payload_json = match serde_json::to_string(&payload) {
        Ok(json) => json,
        Err(e) => {
            tracing::error!("failed to serialize <Command> payload for {item:?}: {e}");
            return Err(());
        }
    };

    match db::decide_and_submit_command(
        pool, /* dispatchers, broadcaster, cache */, &command_type,
        &payload_json, &format!("automation:<slice>"), None, Utc::now(), None,
    ).await {
        Ok(SubmitCommandOutcome::Rejected { reason, .. }) => {
            // Not a failure to retry - decide() looked at the mapped
            // payload and said no. Re-submitting it would just be rejected
            // again, so this item is done - successfully, with no event -
            // but log it, since it's otherwise invisible.
            tracing::warn!("<Command> rejected for {item:?}: {reason}");
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(e) => {
            tracing::error!("submitting <Command> for {item:?} failed, will retry next poll: {e}");
            Err(())
        }
    }
}
```

Non-negotiable rules:

- **`POLL_INTERVAL` gets a comment saying where the number came from**
  (measured latency, the external system's own documented rate limit, a
  guess flagged as one) — an unjustified interval is an invented business
  rule the same way an unjustified threshold is.
- **Secrets come from configuration/environment**, never hardcoded. If the
  key is missing, log and skip this poll — don't panic the whole process on
  a missing credential.
- **The external system failing is a domain case only if `slice.json` models
  it.** If it doesn't, log and leave the item in the queue — don't invent a
  failure event; that's a modelling decision the board owns, not this skill.
  And if the external failure produces a **reassuring** result rather than a
  visible error (a provider that answers "unknown" instead of erroring),
  say so in the worker's own doc comment — that's the class of failure
  nobody discovers in time.

### How many at a time

Firing every pending item at once is exactly when an undocumented rate limit
shows up, in production, on a restart with a deep queue. Pick a ceiling and
write down where the number came from — this is `IN_FLIGHT`/`room_for` in
Step 3's own code above:

```rust
/// One at a time - a burst of four returned 429 while a single call succeeded.
const IN_FLIGHT: usize = 1;

/// Pure - testable with no tokio runtime and no network at all. The only
/// thing `process_pending` above actually asks before launching an item.
fn room_for(in_flight: usize, ceiling: usize) -> bool {
    in_flight < ceiling
}
```

`process_pending`'s own `FuturesUnordered` loop is what raising `IN_FLIGHT`
above 1 actually does here: it keeps up to `IN_FLIGHT` `process_one` calls
running concurrently, in-process, with no `tokio::spawn`/`Arc` needed — never
a second in-memory queue, since an item `room_for` doesn't admit this poll
just stays pending in the durable `Projection` queue itself, so a restart
loses nothing. **On success, pull the next item immediately; on failure,
don't** — chaining retries after a failure turns a rejected call into a
tight loop against someone else's API; let the poll interval be the
backoff. (If your loop instead spawns each item onto its own task — worth
doing once a single call's latency, not just its concurrency, needs to stop
blocking the next poll tick — gate the spawn with a `tokio::sync::Semaphore`
acquired before spawning and released on completion, keeping the same
never-a-second-queue and chain-on-success-only rules.)

---

## Step 4 — Wiring it in

In `src/bin/server.rs`, after `Skilj::builder(...).build()` succeeds and
before `axum::serve(...)` — the file's own doc comment marks the spot:

```rust
tokio::spawn(my_app::<context>::run_<slice>(pool.clone(), /* dispatchers, ... */));
```

One spawn per automation slice. Don't `.await` it directly in `main` — it
runs for the lifetime of the process, same as the axum server itself.

---

## Step 5 — Tests

**File:** `tests/<context_snake_case>.rs`

**The worker isn't tested with the network.** What gets tested:

- The queue `Projection`'s own fold, via `/build-state-view`'s tests — pending
  minus resolved, and that it **stops** returning an item once resolved.
- The response → domain mapping, `to_domain`, as a pure function — pull it
  out precisely so this doesn't need the network.
- The command's own `decide()`, via `/build-state-change`'s tests.
- **`room_for`** — the decision people skip, because it's neither the
  mapping nor the write, so it falls through the gap and ends up the only
  untested logic in the slice, and the one that fails in production rather
  than in the suite. Four cases: nothing in flight launches; something in
  flight (at the ceiling) doesn't; releasing one makes room again; a ceiling
  of 1 never launches a second concurrent item.
- If the slice reads a deployed file: the fixture-matches-the-real-file test
  from Step 2, skipping cleanly when the file is absent.

---

## Step 6 — Quality gate

```
cargo build && cargo test
cargo test <context_snake_case>::
```

---

## Final check against `slice.json`

- [ ] The queue's own `/build-state-view` slice exists and is `Done` (or this
      slice invoked `request-feedback` because it wasn't on the board).
- [ ] **One domain event**, not a "response received" one.
- [ ] Field names are domain names, not the external API's.
- [ ] The raw response is a `technicalAttribute` field, never projected.
- [ ] The event name's preposition matches the board (`With…`/`By…`) exactly.
- [ ] `POLL_INTERVAL`/`IN_FLIGHT` are named constants with a comment saying
      where the number came from.
- [ ] `room_for` is a pure function and has its own tests.
- [ ] Declining to launch leaves the item in the durable queue — no second,
      in-memory one.
- [ ] The worker is spawned once in `server.rs`, not re-spawned per request.
- [ ] Secrets come from configuration, never hardcoded.
- [ ] If the slice reads a deployed file: the fixture is derived from it, with
      a guard test comparing their vocabulary.
