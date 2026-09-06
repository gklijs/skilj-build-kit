---
name: build-state-view
description: Implements a read slice (a read-optimised Projection folded from one or more event types) in Rust with skilj, from a slice.json
---

# Build a read slice

> Before anything else, read the definition at
> `.build-kit/.slices/{Context}/{slice}/slice.json`. Never invent fields that
> aren't there.

> And read `.build-kit/CLAUDE.md`, especially the tag rule — it applies here
> as `keys()`, see below.

> If you haven't already, skim
> `.claude/skills/skilj/references/projection.md` — this skill only covers the
> board-to-Rust translation, not what `Projection`/`sync`/`keys` mean.

---

## What a read slice is

A `Projection` — folded incrementally, one event at a time, never rebuilt from
scratch on an ordinary read. Unlike some other Build-Kits' read models,
**skilj's own `Projection` *is* materialised** (a real `projection_state` row,
updated as events arrive) — it isn't recomputed on every query the way a
"fold on the fly" read model in a framework with no built-in projection
concept would be. That changes what "the query" means here: there's no
per-request fold to write, only a `project()` that runs once per event.

```
skilj (on every write, or a background consumer)
  → BoundedContextEvent::try_from_event(event)
  → Projection::keys(event)         -- which instance(s) this event touches
  → Projection::project(state, event, key)   -- folds into that instance
```

A reader (GraphQL `projection(...)` query, or
`skilj_core::db::get_projection_state(pool, bounded_context, name, key)` for
an in-process caller — an automation's own TODO-queue read, most commonly)
just reads the already-folded row back. There is no `context.ex`-equivalent
wrapper to write for the ordinary case.

---

## Step 1 — Read the `slice.json`

- **`readmodels[]`** and their `fields[]`. Look at:
  - `mapping: "<Event>.<field>"` → **direct copy** inside that event's
    `project()` match arm.
  - `mapping: "derived:…"` → **computed**, either inside `project()` (if one
    event alone determines it) or by the reader after loading the state (if
    it combines several stored fields — skilj's `State` has no
    hook to run extra logic on read, so a `derived:` combining fields belongs
    in the `State` struct's own method, called by whoever reads it).
  - `optional: true` → the event that carries it hasn't folded in yet — model
    it as `Option<T>` in `State` (`#[derive(Default)]` gives you `None`
    automatically), never as a sentinel value.
  - `generated: true` → derived, comes from no event — same rule as `derived:`.
  - `cardinality: "List"` with `subfields` → `Vec<T>` in `State`, where `T` is
    its own `#[derive(..., JsonSchema)]` struct from `subfields[]`.
- **`specifications[]`** — the `given` are events (in board order), the
  `then`/`examples` is the expected folded state.
- **The read model's `description`** — says what's computed vs. copied, and
  usually why. Read all of it before writing anything — confusing this view
  with a similarly-scoped one is the classic mistake (see `keys()` below).

---

## Step 2 — `State` and `Projection`

**File:** `src/<context_snake_case>.rs` — append to the existing context
module, same convention as `build-state-change`.

```rust
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct <Slice>State {
    pub field: Option<String>,
    pub list: Vec<ListItemState>,
}

pub struct <Slice>;

#[auto_register(BOUNDED_CONTEXT)]
impl Projection for <Slice> {
    type State = <Slice>State;
    type Event = <Context>Event;
    const NAME: &'static str = "<Slice>";

    fn consumed_event_types() -> Vec<&'static str> {
        vec!["<EventA>", "<EventB>"]
    }

    fn sync() -> bool {
        true // see "sync vs background" below
    }

    fn keys(event: &Self::Event) -> Vec<String> {
        match event {
            <Context>Event::<EventA>(p) => vec![p.<id_field>.clone()],
            // ...
            _ => vec![], // events this projection doesn't fold touch no instance
        }
    }

    fn project(state: &mut Self::State, event: &Self::Event, _key: &str) {
        match event {
            <Context>Event::<EventA>(p) => { state.field = Some(p.field.clone()); }
            _ => {}
        }
    }
}
```

### `consumed_event_types()` must name every type your `mapping:` values use

**No default, has to be declared, and there's no compiler check that it's
complete** — Rust can't infer which `Self::Event` variants `project()`
actually matches on. Derive the list from every field's `mapping: "<Event>.<field>"`
value: every named `<Event>` must appear here, or registration fails outright
(`UnregisteredEventType` if it names a type that isn't registered at all — but
**a type that's registered elsewhere yet just missing from this list is not
an error, and this is the expensive failure**: the field silently stays `None`/
default forever, and nothing about the running system says so). Worth a test
on this list itself (see Step 4).

### `keys()` — single-instance vs. multi-instance, and the tag-scoping analogy

Defaults to one constant unnamed key — every `Projection` that never
overrides `keys()` has exactly one shared instance folding every consumed
event. Overriding it (as above) creates one independent instance per returned
key, keyed the same way a command's own tag scopes `matching_events` — if
`slice.json`'s read model has an `idAttribute: true` field, that's almost
always the key.

**A TODO queue (a `processors[].dependencies` target — see `build-automation`)
is the one case that stays unkeyed on purpose.** Its consumer is a background
processor, not a request scoped by any one entity's id — leave `keys()` at its
default and say so in the struct's own doc comment, so nobody "fixes" it into
a keyed projection later.

If an event's own `keys()` call returns more than one key (e.g. an event
naming both a sender and a receiver), that one event folds into **each**
returned instance independently, via its own separate `project()` call —
`key` is what tells those calls apart inside `project()` when it matters (most
`project()` bodies, including every worked example so far, take `_key`
unused because each event only ever touches one instance from its own
perspective).

### `sync()` — inline vs. background

- `true` — updates in the same transaction as the event(s) it consumes.
  Reading it back immediately after the command that produced it always
  reflects that command, no polling delay. Use this whenever the slice's own
  `description` implies "the operator expects to see this right away" (a
  balance after a deposit, a status right after submitting).
- `false` (default) — a background consumer, eventually consistent. Fine for
  a view nothing needs read-your-writes for (an analytics rollup, a report).

### Derived fields: computed, never stored redundantly

**Never store on the event something you can derive from what's already
there.** If `mapping: "derived:presence of <Event>"`, that's
`state.status = Status::Resolved` inside that event's own `project()` match
arm — not a separate field copied from the event.

---

## Step 3 — Reading it back

There is no per-slice reader function to write for the ordinary case — the
GraphQL `projection(boundedContext:, name:, key:)` query already exists once
the `Projection` is registered (see `docs/architecture.md`'s GraphQL section
for its exact shape). Only write a wrapper when the reader is in-process (an
automation's own TODO-queue check, most commonly):

```rust
let raw = skilj_core::db::get_projection_state(pool, BOUNDED_CONTEXT, "<Slice>", key).await?;
let state: <Slice>State = raw
    .map(|s| serde_json::from_str(&s))
    .transpose()?
    .unwrap_or_default();
```

`get_projection_state` returns the state as a raw JSON string (or `None` for
"nothing folded for this key yet") — deserialize it yourself; there's no
typed accessor.

---

## Step 4 — Tests

**File:** `tests/<context_snake_case>.rs`

```rust
#[test]
fn <the_specifications_scenario_title_in_snake_case>() {
    let mut state = <Slice>State::default();
    for event in [/* the scenario's `given`, as <Context>Event variants, in board order */] {
        <Slice>::project(&mut state, &event, "<the key the scenario is about, if keyed>");
    }
    assert_eq!(state.field, Some("<the scenario's expected value>".into()));
}
```

- **The pure fold only** — no Postgres, no registration, no HTTP.
- **The data comes from the scenario**: `given` are the events (board order —
  test arrival order too if the view folds flows with no guaranteed
  ordering; two orderings must fold to the same result or the system has a
  race), `then`/`examples` is the expected state.
- **Test the default state** — a fresh `<Slice>State::default()` with no
  events folded has to be meaningful to render (`None`/empty `Vec`, not a
  panic) — the screen (or the automation reading it) will see this the first
  moment after the entity exists.
- **Test that `project()` ignores what isn't its own** — an event type not in
  `consumed_event_types()` (or a variant this projection's own match doesn't
  handle) must leave `state` unchanged.
- **Test `consumed_event_types()` itself** against the `mapping:` values you
  derived it from in Step 1 — this is the one thing the rest of the suite
  can pass green while still being wrong (a missing type just leaves a field
  `None` forever, nothing errors).

---

## Step 5 — Quality gate

```
cargo build && cargo test
cargo test <context_snake_case>::
```

---

## Final check against `slice.json`

- [ ] Every field in `readmodels[].fields[]` exists in `State` or is computed
      by whoever reads it back.
- [ ] `consumed_event_types()` names **every** event type any field's
      `mapping:` value references.
- [ ] `derived:`/`generated: true` fields are computed, never stored
      redundantly.
- [ ] Every `specifications[]` has its own test.
- [ ] There's a test for the empty/default state.
- [ ] `keys()` matches the read model's own scope — keyed by its
      `idAttribute: true` field, or deliberately unkeyed (and documented as
      such) if it's a TODO queue.
- [ ] If the slice has `screens`, you have **not** written an interface — see
      `.build-kit/CLAUDE.md`'s screen-brief template.
