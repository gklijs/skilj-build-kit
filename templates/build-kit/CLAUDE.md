# Blueprint: Rust + skilj (event sourcing, Postgres, DCB)

This is "how we build things here". Not a style guide: it's the contract that
lets an agent implement a slice without anyone having to review where each
type goes or what each thing is called.

Read `src/wallet.rs` before your first slice — it's the framework
(`skilj`/`skilj-core`, published crates) plus one worked example bounded
context. Unlike some other Build-Kits, there's no separate hand-rolled
`decide.ex`/`reader.ex`-style glue layer to read first: `skilj` already *is*
that layer (read → fold → decide → append, DCB optimistic concurrency, tag
indices) — a consuming app only ever implements `EventType`/`CommandType`/
`Projection` trait impls, never the plumbing around them.

## File constraints

- **One Rust module per board *Context*, not per slice.** A board `context`
  (`slice.json`'s own `"context"` field) usually contains many slices over its
  lifetime — each adds new `EventType`/`CommandType`/`Projection` impls, and
  often new variants on that context's own event enum, to the *same* file
  (`src/<context_snake_case>.rs`), the way `wallet.rs`'s own `wallet_tag()`
  and `WalletEvent` are already shared by every type in it. **Don't create a
  new file or folder per slice** the way FACT/Elixir-style Build-Kits do —
  skilj's own registration and `BoundedContextEvent` conversion are per
  bounded context, not per slice, and splitting one context across files
  means duplicating its tag helpers and its event enum, which then drift.
- **Strict path:** work inside `src/<context_snake_case>.rs` and
  `tests/<context_snake_case>.rs`. Nothing else, unless the skill you're
  running says so explicitly.
- **Don't touch `Cargo.toml`'s existing dependencies or `src/bin/server.rs`'s
  bootstrap shape** unless asked — adding a `tokio::spawn` for a new
  automation processor is the one routine exception, see `build-automation`.

## Standards

- **Language:** Rust, edition 2021. **Store:** Postgres via `skilj`/`skilj-core`
  (published crates — pin the same minor version already in `Cargo.toml`,
  never a different one per file).
- **Domain names follow the board.** Event/command/projection `NAME` strings,
  and their Rust type names, come from `slice.json` verbatim (PascalCase, no
  spaces) — they're the shared vocabulary with whoever modelled the board.
- **HTTP for anything outside skilj itself:** `reqwest`, added to `Cargo.toml`
  the first time a slice actually needs it (an automation's external call, a
  webhook's nothing-outbound case doesn't need it at all) — never a second
  HTTP crate alongside it.
- **Every `EventType`/`CommandType`/`Projection` is `#[auto_register(BOUNDED_CONTEXT)]`-tagged**,
  copying an existing type's own macro argument in the same file exactly —
  see `.claude/skills/skilj/references/registration.md` for what that macro
  argument actually does and why it's never invented per type.

## Architecture rules

- **All invariants live in `decide()`, which is pure.** No side effects, no
  network, no store access — see `.claude/skills/skilj/references/command-type.md`'s
  own "pure, synchronous, no I/O" section for why: the optimistic-then-locked
  retry may call it twice for one real submission.
- **Every write goes through skilj's own submit path** (`decide_and_submit_command`
  in-process, or `POST /v1/commands/trigger` over REST) — never construct and
  insert an event by hand.
- **Every read goes through a registered `Projection`**, not an ad hoc query
  against the event store — see `.claude/skills/skilj/references/projection.md`.
- **Tags are the only consistency mechanism.** There is no aggregate ID and no
  per-entity stream — `matching_events` is exactly the tag-scoped set
  `tag_mappings()` describes. See
  `.claude/skills/skilj-event-modeling/references/dcb-tags.md` for the
  technique, including the case a single ID can't express (an event/command
  needing more than one tag).
- **`decide()`/`project()` never call each other, and never call out.** A
  `CommandType` names the `EventType`s it can emit as plain strings in
  `EventSpec` — no compile-time link, so a typo is a registration/runtime
  error, not a compile error (`UnregisteredEventType` — see
  `.claude/skills/skilj/references/common-mistakes.md`).

## Building a slice

**Always use the matching skill. Never implement a slice by hand.**
**Every field, event name, command name and business rule comes EXCLUSIVELY
from `slice.json`.** Don't invent anything that isn't there.

0. **Check the `slice.json` is complete before anything else.** If the loop
   wrote it from the *summary* endpoint (a handful of fields, no `fields[]`,
   no `events[]`, no `specifications[]`), it's a stub — reload it via
   `load-slice` (or the `learn-eventmodelers-api` skill's `get_slice_data`
   MCP tool / `/slicedata?contextName=` REST call) before building anything.
   The file existing and parsing is not the same as it being complete.

1. Read `.build-kit/.slices/<context>/<slice>/slice.json`.
2. Work out the shape and call the skill. `sliceType` is `STATE_CHANGE` |
   `STATE_VIEW` | `AUTOMATION` in the schema this kit was built against —
   **don't hard-fail on a value outside that set** (e.g. a `TRANSLATION`
   value seen in some other stacks' own routing logic isn't confirmed to
   exist on every board); fall through to the field-presence checks below
   instead of erroring on an unrecognized `sliceType`:
   - **an inbound external event** — an `events[]` element with
     `context: "EXTERNAL"` (not the slice-level `context`, the *element's*
     own field — see §3 of this kit's `docs/investigation-findings.md` for
     why this is a better signal than parsing `description` prose) →
     `/build-webhook`.
   - non-empty `processors[]` → `/build-automation`.
   - non-empty `readmodels[]` → `/build-state-view`. (Not `queries` or
     `projections` — those field names don't appear in the schema this kit
     was built against; `readmodels` is the real one.)
   - default (has `commands[]`/`events[]`) → `/build-state-change`.
3. Follow the whole skill. Don't deviate.
4. **Verify against `slice.json`**: every command field, every event field and
   every specification must appear in the code.
5. `cargo build && cargo test` (the whole crate compiles cleanly first — a
   warning that would be an error under `#![deny(warnings)]` if the crate
   opts into it counts). Then the slice's own tests only —
   `cargo test <context_snake_case>::`.
6. If it passes: `git commit -m "feat: <Slice Name>"` and set status `Done`.

## The one rule `slice.json` doesn't tell you: tags come from `idAttribute: true`

`slice.json` ships `tags: []` on every element — **they aren't empty, they're
underived.** The rule is mechanical, and it's the direct Rust-level
translation of `idAttribute`:

> Every field with `idAttribute: true` becomes a `TagMapping { key, field }` —
> `key` the field's own name without an `Id`/`_id` suffix (`accountId` →
> `"account"`), `field` the field's own name in the payload (`account_id` once
> it's a Rust struct field, matching `schemars`'s derived JSON Schema).

`checkId`/`sessionId` → `TagMapping{key:"check",field:"check_id"}` and
`TagMapping{key:"session",field:"session_id"}`. Both the `EventType`s an event
maps to *and* the `CommandType` that can emit them need the matching tag —
that symmetry (not the field's mere presence) is what makes `matching_events`
scoped correctly; see `command-type.md`'s own `matching_events` section.

**This matters more than anything else in this kit.** Tags are the query keys
of the whole system: `decide()`'s own `matching_events`, every `Projection`'s
`keys()`, and every cross-slice consistency guarantee all depend on them. A
made-up or missing tag doesn't fail at registration — it silently changes
which events a decision or a projection sees.

An event/command with **more than one** `idAttribute: true` field needs **more
than one tag** — this is not a smell, it's the DCB pattern that replaces a
saga for facts that must be checked together (`EnrollStudentInCourse` tagging
both `student` and `course` — see `dcb-tags.md`'s own worked example). Don't
collapse a genuinely multi-tag command onto a single tag out of aggregate-ID
habit.

If an element has no `idAttribute: true` field at all, **stop and invoke
`request-feedback`**: an event/command with no tags is either genuinely
global (rare — confirm before assuming it) or missing a field the board hasn't
named yet.

## `pii: true` — the signal `sensitive_fields()` comes from

A `Field` with `pii: true` should become an entry in that type's
`sensitive_fields()` — encrypted at rest, decrypted only for an entitled
reader (see `.claude/skills/skilj/references/event-type.md`'s own
`sensitive_fields` section for the exact `SensitiveField{field, subject_key,
subject_field}` shape and the "which field names the subject" question you
still have to answer by reading the slice's own fields, since `slice.json`
doesn't say *whose* data a `pii` field is). **A field can never be both a tag
and `pii`** — registration rejects the overlap (`SensitiveFieldTagOverlap`);
tag the *subject* field instead if you need both an indexed lookup and an
encrypted value about the same conceptual subject.

## `aggregate`/`aggregateDependencies`/`createsAggregate` — ignored

Vestiges of classic aggregate modeling in the schema this kit was built
against. skilj has no aggregate-ID concept — `idAttribute: true` is the only
signal that matters for tags, whether or not the board's own export also
carries these aggregate-shaped fields. Don't derive anything from them.

## What this kit does NOT do: screens

`slice.json` carries a screen as metadata and prose (`title`, `fields`,
`dependencies`, `description`) — **not as a design.** Whatever HTML lives on
the board never travels in the payload.

If a slice has `screens`: **build the domain, write the screen brief, stop.**
Don't invent an interface — skilj has no UI concept at all; whatever renders
this data is a separate project this kit doesn't scaffold.

### The screen brief

`docs/screens/<slice-in-kebab>.md`, version-controlled (unlike `.build-kit/`,
which is regenerated on every fetch). **Worked example:**
`docs/screens/EXAMPLE-wallet-balance.md`, shipped by this kit — read it for
how much detail is worth writing, then delete it once you have your own.
Template:

```markdown
# Screen: <the screen's title on the board>

Slice `<title>` · node `<screen node id>`.

## How it's entered

<The GraphQL query or REST route that serves this screen's data, with its
exact shape — skilj has no in-process query API a screen calls directly, only
the wire surfaces.>

## What it returns

| field | type | | what it is |
|---|---|---|---|
| `field` | `String` | | <from the board's description, not invented> |

<A `Projection` with no folded events for its key still returns its `Default`
— say what that renders as, never "error".>

## What it sends back

<Only if the slice has a command: the route, its payload shape, and every
`rejectionKind` the screen has to translate — it can't guess them from
`slice.json`.>

## States to render

<Empty/default, submitting, rejected, accepted. Note whether the consuming
`Projection` is `sync()` (immediate read-your-writes) or not (there may be a
polling delay before a just-triggered command's effect is visible).>

## What the board says about this screen

<The screen node's `description`, quoted. It's the design intent, and the only
thing that survives from the board.>

## What the domain does NOT give you

<The most important section. Fields the screen might want that don't exist,
and whether that's deliberate. Stops whoever builds the view from inventing
them.>
```

## Slice shape

```
src/<context_snake_case>.rs        # every EventType/CommandType/Projection
                                    # for this board Context, growing slice
                                    # by slice — shared event enum + tag
                                    # helpers live here too
tests/<context_snake_case>.rs      # integration tests, one #[test] per
                                    # specifications[] scenario (see
                                    # build-state-change's own test section
                                    # for the pure-unit-test-first pattern)
```

**Once you have a context built, read it before the next slice in it.**
Existing code beats these templates: if they diverge, the template is stale.

## Before you start

Read `.build-kit/AGENTS.md` if it exists, to load what earlier iterations
learned. And when you start a slice, invoke `update-slice-status` with
`InProgress` before anything else.

## If something is ambiguous

If `slice.json` is genuinely ambiguous, contradictory, or missing a decision
you need — **don't guess and don't build anyway**. Invoke `request-feedback`
with the specific question. Then stop.

This is an escape hatch, not a routine step: read the whole `slice.json` and
the whole skill first. Most slices are fully specified.
