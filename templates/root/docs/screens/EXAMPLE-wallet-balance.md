# Screen: Wallet Balance

Slice `Wallet Balance` · worked example shipped by skilj-build-kit, not a real
board export.

> **An example, not an empty template.** This kit doesn't build screens (see
> `.build-kit/CLAUDE.md`) — the rendered HTML from the board never travels in
> `slice.json`'s own payload. This is what a screen brief looks like once a
> slice is built; delete it once you have your own.
>
> The section most worth copying is the last one, **"What the domain does NOT
> give you"** — it's what stops whoever builds the actual view from inventing
> fields or asking the domain for things it shouldn't own.

## How it's entered

```rust
// GraphQL/REST, not a Rust function call — skilj has no in-process query API
// a screen would call directly, only the wire surfaces below.
```

The GraphQL `projection(boundedContext: "wallet", name: "Balance", key: "<wallet_id>")`
query (see `docs/architecture.md`'s GraphQL section for the exact query
shape) — a GraphQL Role grant is required; there is no unauthenticated read
path. **Not `GET /v1/events`**: that route returns raw, unfolded `Deposited`/
`Withdrawn` events with no server-side folding at all — it cannot serve a
`Projection`'s state, and re-summing those events client-side would
reimplement `Balance::project()`'s fold outside the registered `Projection`,
exactly what `.build-kit/CLAUDE.md`'s "every read goes through a registered
Projection" rule rules out.

## What it returns

| field | type | | what it is |
|---|---|---|---|
| `balance` | `Int` | | the wallet's current balance — folded from every `Deposited`/`Withdrawn` event tagged with this `wallet_id`, not stored anywhere as a row |

A read model with no events for its key still returns its `Default` — here,
`balance: 0` — never an error. Render that as "wallet not opened yet" if the
screen wants to distinguish it from a real zero balance; the domain itself
makes no such distinction, because there isn't one to make.

## What it sends back

Two commands, `Deposit`/`Withdraw`, each over `POST /v1/commands/trigger` with
a `CommandToken` scoped to that command type. Every rejection kind the screen
has to translate, because it can't guess them:

| kind | when |
|---|---|
| `invalid_amount` | the amount is zero or negative, on either command |
| `insufficient_funds` | `Withdraw` only — the amount exceeds the wallet's current balance |

## States to render

- **No wallet yet** — `balance: 0`, indistinguishable from an opened wallet
  with a zero balance (see above).
- **Submitting** — the command call is a synchronous HTTP round trip;
  there's no separate "pending" state to render.
- **Rejected** — the `rejectionKind`'s message, and the balance is unchanged.
- **Accepted** — `Balance` is `sync: true`, so reading it back immediately
  after a `200 { accepted: true }` always reflects the just-triggered command,
  no polling delay.

## What the board says about this screen

> (In a real slice, the screen node's own `description`, quoted verbatim —
> this worked example has none, since it wasn't exported from a real board.)

## What the domain does NOT give you

The most important part of this document.

- **No transaction history list** — `Balance` folds to one number. A "list of
  past deposits/withdrawals" is a different read model (a different
  `Projection`, or the same events with `cardinality: "List"` state), not this
  one — don't ask this endpoint for it.
- **No account holder name, currency, or opened-date** — nothing in
  `wallet.rs` carries them. If the board's real slice needs them, they come
  from fields on the real `Deposited`/command payloads, not invented here.
- **No "insufficient funds" pre-check before submitting** — unlike some other
  Build-Kits' pattern of a pure `Core.validate/1` a screen can call before
  round-tripping, skilj's `decide()` isn't exposed as a standalone callable
  outside a real command submission; the only way to know if a withdrawal
  will succeed is to submit it and read `accepted`.
