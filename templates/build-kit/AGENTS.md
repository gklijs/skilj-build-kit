# What we learned building

Reusable patterns and traps. Add what you find here, without repeating what's
already written. This file is read **before every slice**: it's the memory
that stops mistakes repeating across iterations.

It ships seeded with what this kit's own authors found investigating skilj and
the eventmodelers platform before any real slice existed here. These aren't
hypotheses about *your* board — they're facts about the framework and the
schema, verified against real source, not guessed.

## The framework

- `skilj`/`skilj-core` already are the "327 lines of glue" some other
  Build-Kits' own frameworks provide by hand (`decide.ex`-equivalent read →
  fold → decide → append, DCB optimistic concurrency, tag indices). There is
  nothing to read in this project's own `src/` except `wallet.rs` — the rest
  of the plumbing lives in the published crates.
- **`decide()` is genuinely pure and directly unit-testable**, no Postgres
  needed: `fn decide(payload: &Payload, matching_events: &[Event]) -> CommandDecision`
  takes nothing but its two arguments. skilj's own `skilj-demo` test suite
  tests it through full HTTP+Postgres integration tests instead (proving the
  *whole* system, registration included) — that's a different, complementary
  goal from a fast pure unit test of one `decide()` call, not a sign the pure
  test isn't the right first thing to write for a new command.
- **One Rust module per board Context, not per slice.** Confirmed by how
  `#[auto_register]`/`BoundedContextEvent` actually work: registration and the
  shared event enum are per bounded context. Splitting one context across
  per-slice files/folders (the FACT/Elixir Build-Kit convention) means
  duplicating tag helpers and the event enum, which then drift.
- **Two in-process write paths exist, no HTTP loopback needed**, when the
  caller lives in the same binary as the rest of the bounded context (the
  normal case for a scaffolded app):
  - `skilj_core::db::decide_and_submit_command(pool, dispatcher, ..., command_type: &CommandType, payload: &str, client_id: &str, ...)` —
    takes a `CommandType` *record* (load it with `db::get_command_type`), not a
    `CommandToken` — no `rest_trigger_allowed` gate applies here, that gate is
    REST-specific.
  - `skilj_core::db::create_and_insert_external_event(pool, ..., adapter: &ExternalEventToken, payload, source_content, source_context, dedupe, ...)` —
    **does** need a real `ExternalEventToken` row (it carries which
    `EventType` to write and the dedupe partition semantics), so an in-process
    webhook handler still mints/loads one at boot, just never sends it over
    HTTP to itself.
  - Reading a `Projection`'s folded state in-process:
    `skilj_core::db::get_projection_state(pool, bounded_context, projection_name, key) -> Option<String>`
    (a raw JSON string of `State` — deserialize it yourself).

## About `slice.json`

- **`tags: []` doesn't mean "no tags", it means "underived"** — see
  `.build-kit/CLAUDE.md`'s own tag section. Most important rule in this kit.
- **`pii: true` on a `Field`** maps to `sensitive_fields()` — a signal FACT's
  own Build-Kit never needed (FACT has no encryption-at-rest concept), so
  don't expect to find this pattern documented anywhere outside skilj's own
  reference skill.
- **`Element.context: "EXTERNAL"`** (not the slice-level `context` — the
  per-element field) is the real signal for "this is a webhook, not an
  automation" — more reliable than parsing `description` prose for who-starts
  language, though the prose is still worth reading for the rest of the
  slice's own rules.
- **The schema this kit was built against has no `"TRANSLATION"` `sliceType`**
  and no `queries`/`projections` fields — only `readmodels`. Some other
  stacks' generic routing logic checks for all of these anyway (copy-pasted
  from an earlier/different schema version, most likely) — don't assume they
  exist on your board without checking a real export first.
- The `Slice.status` enum in that same schema is `Created`/`Done`/`InProgress`
  only, while the platform's own board/loop docs use `Planned`/`Blocked`/
  `Review` too — an unresolved discrepancy this kit's own investigation
  flagged rather than silently picked a side on
  (`docs/investigation-findings.md` in this kit's own repo has the full
  writeup). Handle an unrecognized status defensively, don't hard-fail.

## About tests

- Write the pure `decide()`/`project()` unit test **first**, no Postgres.
  Add a full HTTP+Postgres integration test (`skilj-demo`'s own
  `tests/banking.rs`/`tests/courses.rs` pattern) only when the thing actually
  under test is the wiring itself — registration, DCB conflict handling
  across two real concurrent requests, cross-context routing.
- `#[derive(JsonSchema)]`'s own output can surprise you: `Option<T>` renders
  as a type array (`["string","null"]`), and even a plain unit enum renders
  via `$ref`. If `PayloadDoesNotMatchSchema` fires for a payload you believe
  is correct, check the *registered* schema, not just the Rust struct.

## About registration

- **`InvalidTagMapping`/`InvalidSensitiveField`** almost always means a typo
  in the field name, or a field that isn't a scalar/list-of-scalar leaf (a
  nested object, more than one level of dotted nesting). See
  `.claude/skills/skilj/references/common-mistakes.md` before guessing.
- **`SchemaIncompatible`** on a re-registration means the change isn't
  additive-only (a removed field, optional tightened to required). A new
  field must be `Option<T>`. A genuine reshape needs a new, differently-named
  type (or `upcast_payload` — see `event-type.md` §"Schema versioning").
- **`TagMappingKeyDropped`** — once a tag `key` is live, it can't be
  un-mapped. If a slice's later revision wants to stop tagging a field,
  that's effectively a new type too.

## About claiming slices

- The loop rejects a status change if the slice is already in the target
  status: **that's not an error**, another agent claimed it first. Don't
  retry that slice; move to the next `Planned` one in the current context.

## The screen brief

- If the slice has `screens`, write `docs/screens/<slice>.md` alongside the
  domain code — template in `.build-kit/CLAUDE.md`, worked example at
  `docs/screens/EXAMPLE-wallet-balance.md`.
- **The section that earns its keep is "What the domain does NOT give you."**
  Write it even if the rest ends up short.
- Every `rejectionKind` a command can return always goes in — the screen
  translates it into a message and can't guess it from `slice.json`.
