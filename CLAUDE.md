# CLAUDE.md — agent guidance for autumn-plugin-clickhouse

## Commands

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_BUILD_JOBS=2
export CARGO_TARGET_DIR=~/workspace/autumn-arena/target   # shared with other arena builders; never cargo clean
export TMPDIR=~/workspace/.tmp-cargo                      # /tmp is a 512MB tmpfs

cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

Tests use the `clickhouse` mock server (`test-util` dev-dependency). No live ClickHouse needed.
`examples/analytics.rs` is the only thing that needs a real server (`AUTUMN_CLICKHOUSE__URL`).

## Architecture

| Module | Contents |
|---|---|
| `config` | `ClickHouseConfig`: layered `[clickhouse]` section + validation; `AUTUMN_CLICKHOUSE__*` env overrides; password redacted in `Debug` |
| `error` | `ClickHouseError` (thiserror) + `ErrorKind` + HTTP status mapping + `ClickHouseResultExt::or_http` |
| `client` | `ClickHouse` handle: `ping`, `fetch_all`, `fetch_one`, `execute`, `insert`, `inserter`; `InsertStats` |
| `migrate` | `Migration` + `Migration::create_table` DDL builder + ordered `migrate()` |
| `plugin` | `ClickHousePlugin` (`::new`, `.config_section`, `.config`, `.configure`) + `ClickHouse` extractor |
| `health` | `SELECT 1` check, `IndicatorGroup::HealthOnly`, 1s timeout |
| `metrics` | `clickhouse_{queries,inserts,ddl}_total{outcome}`, `clickhouse_insert_rows_total` |

`lib.rs` re-exports everything and re-exports the `clickhouse` crate for the `Row` derive.

## Rules

- House lints: `unsafe_code = "forbid"`, clippy pedantic + nursery warn, `unwrap_used` /
  `expect_used` / `panic` / `todo` / `unimplemented` deny in production code.
  Tests may use them (`allow-unwrap-in-tests` etc. in `clippy.toml`).
- Unit tests live in `src/<module>/tests.rs`, one submodule per module.
- Docs and comments: ASD-STE100 — short sentences, active voice, simple present tense.
- Every public item needs docs (`missing_docs = "warn"`; CI builds docs with `-D warnings`).
- Never log row data, server messages, or passwords. `ClickHouseError::detail()` is for
  operators, not users.
- Autumn API questions: ground against the docs MCP
  (`~/workspace/skills/autumn-mcp/bin/mcp.py`), never training memory.
- The plugin is the OLAP counterpart to the DuckDB plugin, not a replacement.
  Do not duplicate in-repo plugins (admin, S3 storage, cache-redis, search, media, billing).
