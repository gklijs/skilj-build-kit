# Build kit: Rust · skilj

Turns slices from an [eventmodelers.ai](https://eventmodelers.ai) board into
Rust code, using **event sourcing on [`skilj`](https://codeberg.org/gklijs/SklilJ)**
— Postgres-backed, Dynamic Consistency Boundaries (DCB) instead of classic
aggregate-per-stream, GraphQL and REST surfaces built in.

```
npx @eventmodelers/cli init --stack skilj \
  --git https://codeberg.org/gklijs/skilj-build-kit
```

## What it installs

| | |
|---|---|
| `.claude/skills/build-*` | four skills: state-change, state-view, automation, webhook |
| `.claude/skills/skilj`, `skilj-event-modeling` | skilj's own reference skills, vendored — see "Keeping the vendored skills in sync" below |
| `.build-kit/CLAUDE.md` | the blueprint — "how we build things here" |
| `.build-kit/AGENTS.md` | seeded lessons, read before every slice |
| `.build-kit/lib/*.md` | the ralph loop prompts |
| `Cargo.toml`, `src/` | the framework: one worked bounded context (`wallet`), boot code |
| `docs/screens/` | a worked example of a screen brief |

The shared skills — `connect`, `learn-eventmodelers-api`, `update-slice-status`,
`request-feedback`, `load-slice` — come from the CLI itself.

## skilj + what, exactly

`skilj` already *is* the read/fold/decide/append plumbing some other
Build-Kits' own frameworks hand-roll (optimistic DCB concurrency, tag
indices, a REST+GraphQL wire surface generated automatically once a type is
registered). A consuming app never writes that layer — only
`EventType`/`CommandType`/`Projection` trait implementations.

That also means this kit is thinner than some others in one specific way:
**there's no separate "Context" wrapper layer to write for the ordinary
case.** Once a `CommandType` opts into `rest_trigger_allowed()`, its wire
endpoint already exists. Two places this actually costs something instead of
saving it, both covered explicitly in `build-state-change`/`build-webhook`:

- **A board's "generated" field has no framework-level home.** skilj has
  no hook between "caller submits JSON" and `decide()` seeing it — a
  generated timestamp is already free (`event.metadata.created_at`, stamped
  automatically), but a generated *identifier* either comes from the caller
  (the normal case) or needs a genuinely custom route, not a config flag.
- **A webhook's domain-rejection question has two different real answers**
  depending on whether the board's own specifications expect a business
  rejection at all — `build-webhook`'s own Step 0 is this decision, made
  explicit rather than defaulted.

## Before installing

Unlike a framework needing a separate scaffolding step first (`mix phx.new`,
`rails new`), **this kit's `templates/root/` already is a complete,
runnable `cargo` project** — nothing to create beforehand.

## After installing

Read `INSTALL.md`, which the kit drops in your project root: two steps with
the commands ready to paste. The crate ships as `my_app` because the CLI
copies files without templating, so step one is a one-line `sed`.

## The slice shapes

| `slice.json` has | skill |
|---|---|
| an `events[]` element with `context: "EXTERNAL"` | `build-webhook` |
| non-empty `processors` | `build-automation` |
| non-empty `readmodels` | `build-state-view` |
| default (has `commands`/`events`) | `build-state-change` |

## What this kit does NOT do

**Screens.** `slice.json` carries a screen as metadata and prose — not as a
design. skilj has no UI concept at all; whatever renders this data is a
separate project this kit doesn't scaffold.

When a slice has `screens`, the agent builds the domain, writes a **screen
brief** at `docs/screens/<slice>.md`, and stops. Worked example:
`docs/screens/EXAMPLE-wallet-balance.md`.

## Decisions this kit makes explicit, not silent

Recorded in full, with sources, in `docs/investigation-findings.md` — kept in
this repo as the record of what was actually verified before any skill
content was written, not just a design rationale after the fact:

1. **Tags come from `idAttribute: true`**, mapped to `TagMapping` — the exact
   Rust-level translation of the same rule every Build-Kit shares, but the
   *reason* it matters more here: an event/command with more than one
   `idAttribute` field needs more than one tag (skilj's own DCB pattern,
   replacing what a saga would otherwise need — see `dcb-tags.md`).
2. **`pii: true` → `sensitive_fields()`** — a signal no FACT-based kit needed
   (FACT has no encryption-at-rest concept), found by reading the real
   `event-modeling-spec` JSON Schema directly rather than assumed from prose.
3. **`Element.context: "EXTERNAL"`** is the real signal for webhook-vs-automation
   routing — also only found by reading the real schema, not by parsing
   `description` text for who-starts language.
4. **One Rust module per board Context, not per slice** — the direct
   consequence of skilj's own per-bounded-context registration and shared
   event enum, and a deliberate departure from the per-slice-folder
   convention some other Build-Kits use.

## Keeping the vendored skills in sync

`.claude/skills/skilj/` and `.claude/skills/skilj-event-modeling/` are copied
from [the skilj repository](https://codeberg.org/gklijs/SklilJ) at a point in
time, not a live reference — this kit needs to be self-contained for a
scaffolded project to work without a second, separately-installed skill
package. If skilj's own plugin API changes in a way that affects
`tag_mappings`/`sensitive_fields`/registration, re-copy those two directories
here and re-check every `build-*` skill's own references against the update.

**Known local divergence from upstream**: `skilj/references/projection.md`'s
own `AccountBalance` example uses `saturating_add`/`saturating_sub` instead
of upstream's unchecked `+=`/`-=` (the latter panics or silently wraps once a
folded balance nears `i64::MAX` — the same fix applied to this kit's own
`templates/root/src/wallet.rs`). Re-check this against upstream, and
re-apply the fix, the next time this file is re-copied.

## Provenance

Modeled on [`ortegacmanuel/eventmodelers-elixir-fact-kit`](https://github.com/ortegacmanuel/eventmodelers-elixir-fact-kit),
the reference implementation this kit's own structure was verified against
file-by-file, not assumed from its README's description of itself.

MIT.
