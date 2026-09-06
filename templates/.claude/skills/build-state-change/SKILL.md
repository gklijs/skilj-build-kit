---
name: build-state-change
description: Implements a write slice (a command validated against replayed events, new events emitted) in Rust with skilj, from a slice.json
---

# Build a write slice

> Before anything else, read the definition at
> `.build-kit/.slices/{Context}/{slice}/slice.json`. That file is the **source
> of truth** for every field, event and piece of metadata. Never invent fields
> that aren't there.

> And read `.build-kit/CLAUDE.md`. It carries the tag rule and the `pii`/
> `sensitive_fields()` mapping — both show up below.

> If you haven't already, skim `.claude/skills/skilj/references/command-type.md`
> and `event-type.md` — this skill assumes you know what `decide()`,
> `tag_mappings()`, and `#[auto_register]` are; it only covers the
> board-to-Rust translation, not skilj's own mechanics.

---

## What a write slice is

A command that decides, against tag-scoped history, whether to emit events.

```
caller (REST/GraphQL, or an in-process automation) → skilj's own submit path
                          1. tag-scoped read of matching_events (structural,
                             not a filter decide() applies itself)
                          2. CommandType::decide(payload, matching_events)
                             → CommandDecision::{Accepted{events}, Rejected{reason, kind}}
                          3. optimistic append with a DCB condition,
                             retried on conflict
```

`decide()` is pure — no I/O, no clock, no randomness, safe to call twice for
one real submission. Unlike some other Build-Kits' stacks, **there is no
separate "Context" layer to write**: once a `CommandType` is registered with
`rest_trigger_allowed() -> true`, `POST /v1/commands/trigger` and the GraphQL
`submitCommand` mutation already exist automatically — you don't write an API
handler for the ordinary case. Read Step 3 below before assuming you need one
anyway.

---

## Step 1 — Read the `slice.json`

Pull out:

- **`title`** — the slice name. Names the module, in `snake_case`.
- **`context`** — the bounded context. Becomes (or joins) `BOUNDED_CONTEXT` in
  `src/<context_snake_case>.rs` — **append to that file if it already exists
  for this context**, don't create a new one; see `.build-kit/CLAUDE.md`'s
  "one Rust module per Context" rule.
- **`commands[]`** with their `fields[]`: `name`, `type`, `cardinality`,
  `idAttribute`, `generated`, `optional`, `mapping`, `pii`.
- **`events[]`** — same, plus `dependencies[]` so you know who consumes them.
- **`specifications[]`** — the given/when/then scenarios **with example
  data**. They are the tests, almost literally.
- **Each element's `description`** — it carries the invariants written out in
  prose. It is the source of the business rules; read all of it.

> **Comments**: every element carries `comments: string[]` via its
> `Specification.comments[]`/prose. Use them as hints. If a comment raises an
> **open decision** rather than a hint, don't decide it yourself — invoke
> `request-feedback`. Resolve the ones you consume via the board's comment
> resolve endpoint (`learn-eventmodelers-api` has the exact call).

---

## Step 2 — The payload structs

**File:** `src/<context_snake_case>.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct <Command>Payload {
    pub field_a: String,
    pub field_b: i64,
}

pub struct <Command>;
```

**The fields are the ones in `commands[].fields[]`**, translated by
`Field.type`:

| `slice.json` `Field.type` | Rust field type |
|---|---|
| `String` | `String` |
| `Boolean` | `bool` |
| `Int` | `i32` |
| `Long` | `i64` |
| `Double` | `f64` |
| `Decimal` | `String` by default — `schemars`/serde have no built-in fixed-point type. If the slice's own numbers need real precision (money), that's a real decision: pull in `rust_decimal` (with its own `serde`/`schemars`-compatible wrapper) instead of silently using `f64`. **Flag this rather than picking silently** if the slice's specifications do arithmetic on it — a wrong choice here corrupts every downstream projection. |
| `Date` | `chrono::NaiveDate` |
| `DateTime` | `chrono::DateTime<Utc>` |
| `UUID` | `String` (skilj payloads are plain JSON; there's no UUID newtype requirement, and a tag's value is a JSON scalar leaf either way) |
| `Custom` | a nested struct (its own `#[derive(..., JsonSchema)]`) built from `subfields[]` — see `event-type.md`'s "field shape rule": a tag/sensitive-field path can reach only one level into it |

`cardinality: "List"` → wrap the field in `Vec<T>`. `optional: true` →
`Option<T>`. `technicalAttribute: true` → still a plain field — skilj has no
distinct "technical field" concept, it's a convention, not a type: say in the
struct's own doc comment that it's for forensics/re-derivation and isn't
projected anywhere.

---

## Step 3 — Generated fields: **a genuinely different rule than other Build-Kits'**

`slice.json` marks fields `generated: true` or `mapping: "derived:append instant"`.
Other Build-Kits' own frameworks (FACT/Elixir among them) generate these in an
impure "Context" layer sitting between the caller and the pure core. **skilj
has no such layer for commands** — `POST /v1/commands/trigger` goes straight
from the caller-supplied JSON body to `CommandType::decide(&Payload, ...)`,
with nothing in between. This changes what "generated" means here:

**A generated timestamp is already free — don't add it to the payload at
all.** Every event skilj stores is stamped with `event.metadata.created_at`
automatically, server-side, at submit time (`Utc::now()`, not caller-supplied)
— confirmed directly in `skilj-core::db::decide_and_submit_command`'s own
callers. A board field like `derived:append instant` is exactly this metadata
field, not something `decide()` or the payload needs to carry. **The catch**:
the ordinary `BoundedContextEvent::try_from_event` conversion (the one
`wallet.rs` and every other worked example uses) discards `event.metadata`
when it deserializes `event.payload` into an enum variant — so a downstream
`Projection` that genuinely needs "when did this happen" has to deliberately
carry the timestamp through that conversion (extend the enum variant, e.g.
`Deposited(DepositedPayload, DateTime<Utc>)`) rather than expect it for free.
Most projections don't need this; don't add it unless a specification
actually asks "when" a question.

**A generated identifier has no framework-level home at all.** There is no
place inside skilj's own request path to mint an id before `decide()` sees
it. The normal, no-extra-code answer — and what every worked example
(`wallet_id` included) already does — is: **the caller generates the id
itself** before triggering the command (a UUID minted client-side, exactly
the ordinary DCB/event-sourcing pattern of a caller naming a new entity).
Only if `slice.json`'s own `description` says an id must be **server-minted,
never caller-chosen** does this slice need its own thin `axum` route ahead of
skilj's auto-generated `/v1/commands/trigger`, generating the id and calling
`skilj_core::db::decide_and_submit_command` in-process directly — a real,
narrower reimplementation of the "Context" layer other stacks always have,
built only when the board actually requires it. **If `slice.json` doesn't say
which of these two it means, invoke `request-feedback`** — the two shapes
produce different wire contracts (the standard route vs. a custom one), so
guessing is expensive to undo.

---

## Step 4 — The event structs and their tags

**File:** same `src/<context_snake_case>.rs`, appended after the command.

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct <Event>Payload {
    pub field_a: String,
}

pub struct <Event>;

#[auto_register(BOUNDED_CONTEXT)]
impl EventType for <Event> {
    type Payload = <Event>Payload;
    const NAME: &'static str = "<Event>";
    fn tag_mappings() -> Vec<TagMapping> {
        vec![/* see below */]
    }
}
```

Field translation identical to Step 2's table.

### The tags (a rule `slice.json` doesn't state)

`slice.json` ships `tags: []` on every element. **They aren't empty, they're
underived.**

> Every field with `idAttribute: true` becomes
> `TagMapping { key: "<name without the Id suffix>", field: "<field name>" }`.

`checkId` → `TagMapping{key:"check", field:"check_id"}`. **Both** the
`EventType` and the `CommandType` that can emit it need matching tags — that's
what makes `decide()`'s own `matching_events` scoped correctly (see
`command-type.md`'s own `matching_events` section).

**This matters more than anything else in this skill.** Tags are the query
keys of the whole system. A made-up or missing tag doesn't fail at
registration — it silently changes which prior events a decision sees.

**An element with more than one `idAttribute: true` field needs more than one
tag** — this is the DCB pattern replacing a saga for facts that must be
checked together (see `.claude/skills/skilj-event-modeling/references/dcb-tags.md`'s
own `EnrollStudentInCourse` example, tagging both `student` and `course`).
Don't collapse a genuinely multi-tag command onto one tag out of aggregate-ID
habit.

If an element has no `idAttribute: true` field at all, **stop and invoke
`request-feedback`**: an event/command with no tags can't be scoped, and a
global (untagged) one is rare enough to confirm rather than assume.

### `pii: true` → `sensitive_fields()`

A `Field` with `pii: true` becomes an entry in that type's
`sensitive_fields()` (`.claude/skills/skilj/references/event-type.md`'s own
section has the exact `SensitiveField{field, subject_key, subject_field}`
shape). `slice.json` doesn't say *whose* data it is — read the element's other
fields to find the right `subject_field` (usually the same field an
`idAttribute` tag already names). **A field can never be both a tag and
`pii`** — tag the *subject* field instead if you need both.

### The event type

`NAME` is the event's `title` **verbatim**, PascalCase, no spaces — it's the
string `EventSpec.event_type` names and what `try_from_event` matches on, so
don't embellish it.

---

## Step 5 — `decide()`

```rust
#[auto_register(BOUNDED_CONTEXT)]
impl CommandType for <Command> {
    type Payload = <Command>Payload;
    type Event = <Context>Event;
    const NAME: &'static str = "<Command>";
    fn tag_mappings() -> Vec<TagMapping> {
        vec![/* same shape as Step 4 */]
    }
    fn rest_trigger_allowed() -> bool {
        true // false only if `slice.json` says this command is never
             // externally triggerable (rare — confirm, don't guess)
    }
    fn decide(payload: &Self::Payload, matching_events: &[Self::Event]) -> CommandDecision {
        // fold matching_events by hand into whatever state the decision needs
        if /* rejection condition from slice.json's SPEC_ERROR scenarios */ {
            return CommandDecision::Rejected {
                reason: "<human-readable, from the board's own scenario title>".into(),
                kind: "<short_stable_label>".into(),
            };
        }
        CommandDecision::Accepted {
            events: vec![EventSpec {
                event_type: "<Event>".into(),
                payload: serde_json::json!({ /* field: payload.field, ... */ }),
            }],
        }
    }
}
```

### What `decide()` folds

**Only what the decision needs**, by hand, from `matching_events` — there is
no separate `query`/`initial_state`/`apply_event` split the way some other
stacks' cores have; skilj's DCB tags already narrow `matching_events` to the
tag-scoped set before `decide()` ever runs, so folding "everything for this
entity" and folding "everything the decision needs" are usually the same
loop.

### All validation lives in `decide()`

The board's `SPEC_ERROR` scenarios (`specifications[].when[].type ==
"SPEC_ERROR"` in the schema) are tested against `decide()` directly, pure, no
Postgres. Put every check there — shape checks included — rather than
inventing a separate validation layer: there isn't one to put it in, and
splitting validation between `decide()` and something else means the board's
own error scenarios can't be tested against the thing that actually runs.

Error reasons are **domain strings** in `reason`; `kind` is the short,
machine-readable label a caller branches on (`"insufficient_funds"`, not
`"Insufficient Funds"`) — always both together (see
`command-type.md`'s own `CommandDecision::Rejected` section).

### Thresholds get justified

If you need a constant `slice.json` doesn't give you, write in a comment
**where it comes from and in what units**. An unjustified threshold is an
invented business rule.

---

## Step 6 — Register the event enum variant

**One shared enum per bounded context** (`.build-kit/CLAUDE.md`'s own file
convention) — add this event's variant and match arm to the *existing*
`<Context>Event` enum in the same file, never a new enum:

```rust
pub enum <Context>Event {
    // ...existing variants...
    <Event>(<Event>Payload),
}

impl BoundedContextEvent for <Context>Event {
    fn try_from_event(event: &Event) -> Option<Result<Self, serde_json::Error>> {
        match event.event_type.name.as_str() {
            // ...existing arms...
            "<Event>" => Some(serde_json::from_str(&event.payload).map(<Context>Event::<Event>)),
            _ => None,
        }
    }
}
```

`EventSpec.event_type` in Step 5 is this event's `NAME` as a **plain string**
— there's no compile-time link between a `CommandType` and the `EventType`s it
emits, so a typo here is a registration/runtime `UnregisteredEventType` error,
not a compile error (`.claude/skills/skilj/references/common-mistakes.md`).

---

## Step 7 — Tests

**File:** `tests/<context_snake_case>.rs`

**Write the pure `decide()` unit test first — no Postgres needed.** `decide()`
takes nothing but `&Payload`/`&[Event]`, so call it directly:

```rust
#[test]
fn <the_specifications_scenario_title_in_snake_case>() {
    let matching_events = vec![/* the scenario's `given`, as <Context>Event variants */];
    let payload = <Command>Payload { /* the scenario's `when` example data, literally */ };
    let decision = <Command>::decide(&payload, &matching_events);
    assert!(matches!(decision, CommandDecision::Accepted { .. } /* or Rejected{kind: "...", ..} */));
}
```

**One test per `specifications[]` entry, named after its literal title** (in
`snake_case`) — that way a board scenario and its test are recognizable at a
glance. **The data comes from `specifications[].given/when/then[].fields[].example`**,
literally — don't invent values; the board's are usually measured. A
`SPEC_ERROR` scenario asserts `CommandDecision::Rejected{kind: "...", ..}` with
the board's own `kind`.

**Add a full HTTP+Postgres integration test** (`skilj-demo`'s own
`tests/banking.rs`/`tests/courses.rs` pattern — mint a `CommandToken`, POST to
the router, assert on the projection) only when what's actually under test is
the wiring itself: two concurrent commands racing a DCB conflict, a
projection's `sync()` read-your-writes guarantee, cross-context routing. Most
slices don't need one.

---

## Step 8 — Quality gate

```
cargo build && cargo test
cargo test <context_snake_case>::   # this slice/context only
```

Never commit a crate that doesn't build clean.

---

## Final check against `slice.json`

- [ ] Every field in `commands[].fields[]` is in the payload struct.
- [ ] Every field in `events[].fields[]` is in the event payload struct.
- [ ] No invented fields — except a generated timestamp, which isn't a field
      at all (Step 3), and a generated id, only when Step 3's "server-minted"
      case genuinely applies.
- [ ] Every `idAttribute: true` produces its tag, symmetric on command and
      event.
- [ ] Every `pii: true` field is in `sensitive_fields()`, on its subject field
      rather than overlapping a tag.
- [ ] Every `specifications[]` has its own test.
- [ ] `decide()` has no side effects: no `Utc::now()`, no `Uuid::new_v4()`, no
      `reqwest`, no direct DB access.
- [ ] `try_from_event`'s match has a final `_ => None` arm.
- [ ] The event/command variant was added to the *existing* `<Context>Event`
      enum in this context's file, not a new one.
- [ ] If the slice has `screens`, you have **not** written an interface: you
      have written the **screen brief** at `docs/screens/<slice>.md`, as
      `.build-kit/CLAUDE.md` requires. Include every `rejectionKind` — the
      screen translates them and can't guess them.
