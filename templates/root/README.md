# my_app

A working [skilj](https://codeberg.org/gklijs/SklilJ) app, scaffolded by
[skilj-build-kit](https://codeberg.org/gklijs/skilj-build-kit) for
[eventmodelers.ai](https://eventmodelers.ai). `skilj` is a Rust library for
building event-sourced applications backed by Postgres — Dynamic Consistency
Boundaries (DCB) instead of classic aggregate-per-stream, GraphQL and REST
surfaces built in. See its own README for what that means and why.

**Read `INSTALL.md` first** — two steps with the commands ready to paste.

This project ships one worked example bounded context, `wallet`
(deposit/withdraw, one `Balance` projection — see `src/wallet.rs`), to give
an agent building your first real slice something concrete to pattern-match
against. Delete it once you have your own.

## Run it

```sh
export DATABASE_URL=postgres://user:pass@localhost:5432/my_app
cargo run --bin server
```

## Docker

```sh
docker build -t my_app .
docker run --env DATABASE_URL=postgres://user:pass@host:5432/my_app -p 8080:8080 my_app
```

The image is `FROM scratch` — no shell, no package manager, nothing besides
the binary and a CA bundle. `docker exec ... sh` won't work; use `docker logs`
instead.

## Where to go from here

- `src/wallet.rs` — the `EventType`/`CommandType`/`Projection` trio, and the
  "one Rust module per board Context, not per slice" convention this whole
  kit follows. Read its own doc comment before your first slice.
- `.build-kit/CLAUDE.md` — "how we build things here": the blueprint every
  `build-*` skill assumes.
- `.claude/skills/skilj/` and `.claude/skills/skilj-event-modeling/` — skilj's
  own reference skills (vendored into this kit — see this repo's own README
  for the sync policy), for the Rust-level mechanics (`tag_mappings`,
  `#[auto_register]`, schema evolution) the `build-*` skills assume rather
  than re-teach.
- `src/bin/server.rs` — the bootstrap shown here is a shortcut (seeds a `Role`
  directly), not the intended production flow. See
  [`docs/architecture.md` §5/§6](https://codeberg.org/gklijs/SklilJ/src/branch/main/docs/architecture.md)
  in the skilj repository for the real bootstrap secret / superadmin /
  GraphQL admin console flow.
