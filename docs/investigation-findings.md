# Investigation findings: a skilj Build-Kit for eventmodelers.ai

Written before any `stack.json`/`SKILL.md` content exists in this repo. Everything
below was verified against real, fetched sources — not inferred from the concept
of a "Build-Kit" or from prose summaries. Sources are named inline so a later
reader can re-check them.

Corrects and supersedes the plan in `skilj`'s own `docs/architecture.md` §41
("A skilj Build-Kit for eventmodelers.ai: plan, not yet built"), which was written
before this repo existed and before a real `slice.json` schema had been found.

> **Section numbers below are a point-in-time citation, not a stable
> address.** Every `architecture.md §NN` reference in this document names the
> section as it stood in `gklijs/SklilJ` commit `4b5b834` (see "Sources
> read" below) — the upstream doc has since deleted and renumbered that
> section (as of upstream commit `87db8fe`, `§41` now names the unrelated
> `skilj-amqp` bridge). Don't follow a live upstream link by number; if you
> need the content this document cites, check this doc's own quoted excerpts
> first, or search the live doc by heading text instead of section number.

## Sources read

- `github.com/ortegacmanuel/eventmodelers-elixir-fact-kit` (cloned fresh) — the one
  real, working Build-Kit example. All four `templates/.claude/skills/build-*/SKILL.md`,
  `stack.json`, `templates/build-kit/{AGENTS,CLAUDE}.md`, `templates/build-kit/lib/{prompt,backend-prompt}.md`,
  `templates/build-kit/refresh-slices.py`, and every file under `templates/root/`.
- `@eventmodelers/cli` on npm (real published package, latest `1.0.45` at the time of
  writing) — downloaded the tarball directly and read `cli.js` itself (the actual
  `stack.json` validation logic), `shared/build-kit/lib/ralph.js` (writes `slice.json`
  to disk), `shared/skills/{load-slice,learn-eventmodelers-api}/SKILL.md`, and the
  CLI's own built-in `stacks/{axon,blank,cratis-csharp}` for comparison.
- `dilgerma/event-modeling-spec`'s `eventmodeling.schema.json` — the canonical JSON
  Schema `learn-eventmodelers-api/SKILL.md` names as the source of truth for slice
  data. Fetched and read in full. **This is the first real `slice.json`-shaped schema
  found anywhere in this investigation** — every prior source (including this
  project's own prior architecture-doc entry) only described the shape in prose.
- `gklijs/SklilJ` (already checked out locally at `/home/gklijs/projects/skilj`,
  commit `4b5b834`) — `.claude/skills/skilj/references/*`,
  `.claude/skills/skilj-event-modeling/references/dcb-tags.md`,
  `templates/skilj-template/` in full, `docs/architecture.md` §41.

## 1. What a Build-Kit mechanically is — confirmed, with the exact validation rules

A git repo consumed via `npx @eventmodelers/cli init --stack <name> --git <repo>`.
`cli.js` itself (not just the README) gives the real contract:

```
stack.json                    # { label: string (required),
                               #   kitSubdir?: safe-relative-path (default "build-kit"),
                               #   useShared?: boolean (default true),
                               #   needsBoardId?: boolean (default true) }
templates/.claude/...          # required
templates/root/                # required
templates/<kitSubdir>/         # required — "build-kit" unless stack.json overrides it
```

`useShared: true` (the default, and what every real stack uses) means the CLI
copies its own `shared/build-kit/*` — `ralph.js`/`ralph-claude.js`/`ralph-ollama.js`/
`ralph.sh`, `realtime-agent.js`, `code-export.mjs`, `lib/agent.sh`, `lib/ollama-agent.js`,
`package.json`, `README.md` — into the installed kit **first**, then overlays our
`templates/<kitSubdir>/*` on top for the real per-stack differences (the two
ralph-loop prompt `.md` files, mainly). We never author the shared files.

The four generic skills — `connect`, `learn-eventmodelers-api`, `update-slice-status`,
`request-feedback`, plus `load-slice` — also come from the CLI's own `shared/skills/`,
confirmed directly in the tarball. Not our concern either.

The CLI's own built-in stacks are `node`, `supabase`, `axon`, `cratis-csharp` — no
Elixir, no Rust. Community stacks (elixir-fact, and ours) are installed exclusively
via `--git`, and `--stack <name>` accepts an arbitrary name in that case (it's just
a label — the actual template content comes from the clone). This confirms the
invocation in the prompt (`init --stack skilj --git <this-repo>`) is exactly right,
not a guess.

## 2. The four `SKILL.md` files — all four now verified, not three-guessed-one-real

| Skill | What the real file actually says |
|---|---|
| `build-state-change` | Command struct → `core.ex` (pure `query/initial_state/apply_event/execute`, **all validation lives here, shape checks included** — the board's `SPEC_ERROR` scenarios are tested against pure `Core` and never go through `Context`) → `context.ex` (impure: generates id/timestamp — the **one** authorised deviation from "nothing not in `slice.json` is in the code" — then calls the write path). Maps to `CommandType::decide()` almost exactly, including the "pure, callable twice with different inputs" constraint. |
| `build-state-view` | Two files only: `core.ex` + `context.ex`. Folded on every read, never materialised. `query/1` must name **every** event type any field's `mapping:` references — the "expensive failure" is a type silently missing from the query, which doesn't error, it just leaves a field `nil` forever. Maps to `Projection`, including the "a TODO queue isn't scoped by tag" exception mapping cleanly onto `Projection::keys()`'s default single-unkeyed-instance behaviour. |
| `build-webhook` | **Explicitly "a write slice with a different trigger."** Build the same four write-slice files first (`/build-state-change`), then add a controller + a **plug** that verifies an HMAC signature against the **raw** body (never in the controller — body-parsing has already consumed the raw bytes by then). Always returns **200 even on domain rejection** (a 4xx/5xx just makes the external system retry a body it's already established is bad); 401 is reserved for signature failure alone. The decision rule for "is this a webhook, not an automation" is **who starts**: the external system arriving unprompted, vs. us polling. |
| `build-automation` | **This is where the prior plan (architecture.md §41) was substantively wrong** — see §5 below. |

## 3. A real `slice.json` schema — found

`eventmodeling.schema.json` (JSON Schema, draft-07). Relevant `$defs`:

- **`Slice`**: `id`, `status` (enum, see caveat below), `index`, `title`, `context`,
  `sliceType` (enum `STATE_CHANGE | STATE_VIEW | AUTOMATION` — **no `TRANSLATION`**,
  see caveat), `commands[]`, `events[]`, `readmodels[]`, `screens[]`, `screenImages[]`,
  `processors[]`, `tables[]`, `specifications[]`, `actors[]`, `aggregates[]` (string list).
- **`Element`** (shape of every entry in `commands`/`events`/`readmodels`/`screens`/`processors`):
  `id`, `title`, `type` (enum `COMMAND | EVENT | READMODEL | SCREEN | AUTOMATION`),
  `fields[]`, `dependencies[]` (required), `description`, `tags[]`, `context`
  (enum **`INTERNAL | EXTERNAL`** — see finding below), `domain`, `modelContext`,
  `slice`, `aggregate`, `aggregateDependencies[]`, `apiEndpoint`, `service`,
  `createsAggregate`, `triggers[]`, `sketched`, `prototype`, `listElement`.
- **`Field`**: `name`, `type` (enum `String|Boolean|Double|Decimal|Long|Custom|Date|DateTime|UUID|Int`),
  `example`, `subfields[]`, `mapping`, `optional`, `technicalAttribute`, `generated`,
  `idAttribute`, **`pii`**, `schema`, `cardinality` (`List|Single`).
- **`Specification`**: `id`, `title`, `given[]`/`when[]`/`then[]` (arrays of
  `SpecificationStep`), `comments[]`, `linkedId`, `vertical`, `sliceName`.
- **`SpecificationStep`**: `title`, `id`, `type` (enum `SPEC_EVENT | SPEC_COMMAND | SPEC_READMODEL | SPEC_ERROR`),
  `tags[]`, `examples[]`, `fields[]`, `index`, `specRow`, `linkedId`, `expectEmptyList`.
- **`Dependency`**: `id`, `type` (`INBOUND|OUTBOUND`), `title`, `elementType`.
- **`Actor`**: `name`, `authRequired`. **`Table`**: `id`, `title`, `fields[]`.

### Two genuinely new, previously-unknown signals

- **`Element.context: "INTERNAL" | "EXTERNAL"`** — a machine-readable field nobody
  in this investigation (including the prior architecture.md entry) had found. This
  is a much better signal for "is this slice a webhook" than parsing `description`
  prose for who-starts language: it maps directly onto skilj's own
  `EventType::external_creation_allowed()`.
- **`Field.pii: boolean`** — maps directly onto skilj's `sensitive_fields()`/
  `SensitiveField`. FACT (the elixir kit's store) has no encryption-at-rest concept,
  so their kit never needed this signal and never mentions it — skilj does, and this
  is the field that should drive it.

### Discrepancies to flag, not silently resolve

- **`sliceType` has no `"TRANSLATION"` value** in this schema, yet the elixir kit's
  own `CLAUDE.md` and the CLI's generic `blank`/built-in-stack routing logic both
  branch on `sliceType === "TRANSLATION"`. Either that value exists in practice but
  postdates this schema snapshot, or it's dead logic copy-pasted stack to stack.
  Don't invent handling for it without checking against a live board export.
- **Only `readmodels` is a real field.** `queries` and `projections` — both of which
  appear in the generic routing logic (`blank`'s `CLAUDE.md`, the CLI's own
  `shared`-level `prompt.md`) as alternative field names to check — are **not**
  in the schema. Use `readmodels` only.
- **`status` enum here is `Created | Done | InProgress`** — but the ralph-loop docs
  everywhere else (including this schema's own ecosystem) use `Planned`/`Blocked`/
  `Review` too. This schema may be the *import-config* shape rather than the live
  board-export vocabulary. Unresolved — don't reconcile it by guessing.
- `Element` also carries `aggregate`/`aggregateDependencies`/`createsAggregate` —
  vestiges of classic aggregate modeling that skilj's own DCB tags supersede. Our
  own `build-state-change`/`build-state-view` skills should say explicitly that
  these are ignored; `idAttribute: true` is the only signal that matters for tags,
  aggregate-shaped or not.

## 4. skilj's own side — no surprises, cleanly maps

`.claude/skills/skilj/references/*` confirms `CommandType`/`EventType`/`Projection`
shapes, `tag_mappings()`, `sensitive_fields()`, registration via `#[auto_register]`,
exactly as documented. `dcb-tags.md`'s multi-tag example (`courses.rs`, tagging both
`student` and `course`) is real and has a real concurrency test, not aspirational —
worth citing directly in our own `build-state-change/SKILL.md` for the "an event/tag
list can have more than one tag" case the elixir kit's own tags-are-single-value
examples never show. `templates/skilj-template/src/wallet.rs` is a complete, current,
runnable worked example (event/command/projection trio) — good enough to lift
patterns from directly for our own `templates/root/`.

## 5. Correction: `build-automation`'s real shape

The architecture.md §41 mapping proposed two skilj mechanisms as the answer —
`EventType::system_triggered_allowed` (cron) for a time-based automation, or
`CrossContextRoute` (§36) for an event-reacts-to-event one. **The real
`build-automation/SKILL.md` describes neither.** It's dominated by a third shape:
a GenServer that polls a TODO-queue **read model** (itself a separate `/build-state-view`
slice), calls an external system inside a supervised `Task` (never inline — a
blocking call in `handle_info` blocks the whole poll), and writes its own command
based on the result — with an explicit anti-corruption rule ("our domain fact
leads, the external response fills it in — never model 'we received this'") and a
mandatory, justified in-flight concurrency ceiling.

That's not cron-scheduled and not a skilj-internal cross-context reaction — it's
ordinary application code: a background worker that reads a skilj `Projection`
(the queue), calls out, and submits a `CommandType` via a `CommandToken`, entirely
outside skilj's own built-in primitives. The closest existing analogue in this
codebase is `skilj-template`'s own `server.rs` bootstrap code, just running as a
long-lived worker loop instead of one-shot startup code. **Decide this explicitly
before writing `build-automation/SKILL.md`** — `system_triggered_allowed` remains
the right answer only for the genuinely time-only case (no external queue, no
external call), which the elixir kit's own automation skill doesn't actually cover.

## 6. `AGENTS.md` vs `CLAUDE.md` — genuinely different files, not duplicates

Diffed directly: `CLAUDE.md` (233 lines) is the static blueprint ("how we build
things here"). `AGENTS.md` (109 lines) is a **living, seeded lessons file** — "read
before every slice," and the loop appends to it as it learns things — despite
`stack.json`'s own doc-comment phrasing in this project's architecture.md lumping
them together as one "top-level orchestration prompt." The CLI's *built-in* stacks
(`axon`, `blank`, `cratis-csharp`) instead put this living file at
`templates/build-kit/lib/AGENT.md` (singular, no top-level `AGENTS.md` at all) —
two different conventions between "community `--git` kit" and "built-in stack."
Leaning toward the elixir kit's convention (top-level `AGENTS.md`) since it's our
closest structural sibling, but this is a real decision, not settled by this
investigation.

## 7. skilj's REST wire contract — resolves open item 2

Read `docs/architecture.md` §7 and `skilj-rest/src/{auth.rs,routes/mod.rs}` directly
(not just the plugin-API skill, which explicitly excludes this). Six routes, each
gated by a distinct `AccessToken` variant presented as `Authorization: Bearer <id>.<secret>`:

```
POST /v1/events/external        -- ExternalEventToken  (ExternalEventIngestion)
POST /v1/events/direct          -- DirectCreationToken  (DirectEventCreation)
GET  /v1/events                 -- EventReadToken       (client-tracked fetch)
GET  /v1/events/consume         -- EventReadToken       (server-tracked, auto|manual ack)
POST /v1/events/consume/ack     -- EventReadToken       (manual-ack only)
POST /v1/commands/trigger       -- CommandToken         (CommandTrigger)
```

**For `build-webhook`**: `POST /v1/events/external` is the real target. Body is
`{ payload, sourceContent, sourceContext? }`, response `201 { sequence }` (or
`{ sequence: null, redelivered: true }` for a deduped resubmission — never an
error). Optional `dedupe: { partitionKey, sequence }` is **both-or-neither by
construction** (a `DedupeRequest` sub-struct, not two loose optional fields) —
confirms architecture.md §41's guess that a dedupe mechanism exists for a
partitioned/ordered inbound source, and gives the exact wire shape.

> **Update, later than the rest of this investigation**: `dedupe` turned out
> to still be "Unreleased" per the skilj CHANGELOG at the time this kit's
> `build-webhook/SKILL.md` was implemented and compiled — the published
> `skilj-core = "0.0.4"`'s in-process `create_and_insert_external_event`
> (the function the *in-process* path below actually calls, as opposed to
> this REST route) has no `dedupe` parameter yet. Whether the REST route
> itself already accepted it ahead of the crate isn't re-verified here; treat
> this paragraph as the schema/design intent, and `build-webhook/SKILL.md`
> Step 4b as the currently-compiling reality.

This is a
genuinely different design from the elixir kit's own webhook pattern (HMAC-signature
plug in front of a Phoenix controller): here the signature verification (if the
external system signs its webhooks) still has to happen in the consuming app's own
axum handler *before* forwarding to `POST /v1/events/external` — skilj's REST layer
itself has no signature-verification concept, only token-based auth. So our
`build-webhook/SKILL.md` needs two layers: the app's own thin axum handler (verifies
the external signature, translates the body to the registered `EventType`'s payload
shape), then a call to skilj's own REST endpoint (or, if the webhook handler lives in
the same binary as the rest of the app — the normal case — a direct in-process call,
see below, is simpler than looping a request back through localhost).

**For `build-automation`**: two real in-process (no HTTP, no token) APIs exist,
confirmed directly from `skilj-core::db`, and already used by the very thing
architecture.md §41 named as the alternative mechanism (`CrossContextRoute`, §36),
which calls both of these directly rather than through REST/GraphQL:

- `skilj_core::db::get_projection_state(pool, bounded_context, projection_name, key) -> Option<String>` —
  reads a `Projection`'s folded state (a JSON string of `Self::State`) directly. This
  is how an automation's TODO-queue read happens in-process — no `EventReadToken`
  needed at all when the worker lives in the same binary as the bounded context.
- `skilj_core::db::decide_and_submit_command(pool, dispatcher, projection_dispatcher, snapshot_dispatcher, event_broadcaster, event_cache, command_type, payload, client_id, encryption_key, now, idempotency_key)` —
  the same optimistic-then-locked-retry submit path `POST /v1/commands/trigger` and
  the GraphQL `submitCommand` resolver both call. An in-process automation worker
  calls this directly too, with its own synthetic `client_id` — no `CommandToken`
  required.

This resolves the "who calls what" half of the §5 correction below: a `build-automation`
worker that lives in the same Rust binary as the rest of the bounded context (the
normal case for a scaffolded app) never touches the REST surface at all — it's
`get_projection_state` + external call + `decide_and_submit_command`, all in-process.
`CommandToken`/`ExternalEventToken` only matter when the caller is a genuinely
separate process (a remote workflow, an AI agent, another service) — worth being
explicit about which case a given automation slice actually is before assuming REST.

## Decisions (asked, not assumed)

1. **`sliceType`/`status` discrepancy** — no live board export available. Proceeding
   on the schema as the ground truth for field *names*, but every SKILL.md that
   branches on `sliceType`/`status` handles unrecognized values defensively (checks
   known enum members, doesn't hard-fail on e.g. a `TRANSLATION` value the schema
   doesn't list) rather than assuming the schema is exhaustive.
2. **`AGENTS.md` location** — top-level `.build-kit/AGENTS.md`, matching
   `eventmodelers-elixir-fact-kit` exactly (our closest structural sibling), not the
   CLI's own built-in `lib/AGENT.md` convention.
3. **`build-automation` shape** — follow the elixir kit's queue-poller shape. A real
   in-process worker (`get_projection_state` + external call + `decide_and_submit_command`,
   all per §7) belongs in `templates/root/`, not just a doc note pointing at
   `system_triggered_allowed`.

## Status

Investigation complete. Drafted `stack.json`, the four `SKILL.md` files,
`templates/build-kit/{CLAUDE,AGENTS}.md` + `lib/{prompt,backend-prompt}.md`,
vendored skilj's own two reference skills, and `templates/root/`.

## §8. Two real defects found by actually compiling `templates/root/`, not by reading

`skilj-template`'s own shipped `Cargo.toml`/`server.rs` (read in §"skilj's own
side" above) turned out to be **stale against the currently-published `0.0.4`
crates** — caught by running `cargo check`/`cargo build` against this kit's
own scaffold, not by re-reading source:

1. **`schemars` major-version mismatch.** `skilj-template`'s `Cargo.toml` pins
   `schemars = { version = "0.8", features = ["chrono"] }`, but `skilj-core
   0.0.4` itself now depends on `schemars = "1"` with features `["chrono04",
   "uuid1"]` (schemars 1.x renamed the feature and versions it per upstream
   crate). Two different major versions of the `JsonSchema` trait in the
   dependency graph means a locally-derived `impl JsonSchema` satisfies
   neither bound skilj's own traits require — a real compile error
   (`E0277`), not a lint. Fixed by matching skilj-core's own pin exactly.
2. **`create_command_token`'s signature gained a `scope: Option<String>`
   parameter** (5th of 6, before `now`) since `skilj-template`'s own
   `server.rs` was last written — presumably the cross-tenant `CommandToken.scope`
   mechanism `docs/architecture.md` describes elsewhere. Fixed by passing
   `None` (this bootstrap mints an unscoped, org-wide admin token).

Both are recorded here because they're exactly the kind of drift a future
reader of *this* kit needs to re-check for: **`skilj-template` itself may
still be stale** even after this fix (this kit only fixed what it needed for
its own scaffold to compile, it didn't go back and fix `skilj-template`
upstream) — don't assume `skilj-template`'s own shipped files are current
without checking, the same way this investigation initially didn't.
