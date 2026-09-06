# Installing the framework

Two steps after `npx @eventmodelers/cli init --stack skilj --git <this repo>`.

**This file is meant to be read and deleted.**

## 1 · Rename the crate

The CLI copies files without templating, so this scaffold ships as the
concrete crate `my_app`. `server.rs` refers to it by name (`my_app::register`,
`my_app::wallet::BOUNDED_CONTEXT`) — Rust doesn't resolve an own-crate path any
other way, so both the package name and every `my_app::` reference have to
change together:

```bash
NAME=my_checkout_service   # ← change this to yours (snake_case — Cargo turns
                            #   a hyphenated package name into this same form
                            #   for `extern crate` purposes anyway, so naming
                            #   the package in snake_case from the start
                            #   avoids a mismatch between Cargo.toml's `name`
                            #   and every `my_app::` path in the code)
grep -rl 'my_app' Cargo.toml src .claude/skills .build-kit/CLAUDE.md \
  | xargs sed -i "s/my_app/$NAME/g"
```

## 2 · Postgres

```bash
export DATABASE_URL=postgres://user:pass@localhost:5432/$NAME
cargo run --bin server
```

`db::migrate` runs automatically on every boot — there's no separate
`fact.setup`-style alias to remember, unlike some other Build-Kits' stores.
The first boot creates the `wallet` bounded context, a fresh admin `Role`, and
prints a `CommandToken` for each of `Deposit`/`Withdraw` — use one to confirm
the server actually works before starting your first real slice:

```bash
curl -H 'authorization: Bearer <id>.<secret>' -H 'content-type: application/json' \
     -d '{"payload":{"wallet_id":"w1","amount":100}}' \
     http://localhost:8080/v1/commands/trigger
```

**The bootstrap in `server.rs` is a shortcut, not the intended production
flow** — see its own doc comment, and `docs/architecture.md` §5/§6 in the
[skilj repository](https://codeberg.org/gklijs/SklilJ) for the real bootstrap
secret / superadmin / GraphQL admin console flow, and for wiring a real
`identity_provider` so GraphQL's Role-based auth (not just the REST command
tokens this scaffold sets up) actually works.

## Done

```bash
cargo build && cargo test
```

Once it's green, **delete this file, delete `src/wallet.rs` and its
registration in `src/lib.rs`** (keep a copy around via `git show` on this
commit if you want to refer back to the worked example later — every
`build-*` skill's own docs point at it by name), and start marking slices
`Planned` on the board.
