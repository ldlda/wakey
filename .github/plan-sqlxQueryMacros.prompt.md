# Plan: SQLx Query Macro Adoption

## Summary

Move control-plane SQL from unchecked `sqlx::query(...)` calls toward `sqlx::query!` / `query_as!` where the SQL shape is static.

This gives compile-time checking for table/column names and Rust value types. Dynamic filter builders can stay on `QueryBuilder`.

## Offline Setup

Normal builds should not need a live database:

```text
SQLX_OFFLINE=true
```

`.env` is ignored, so copy:

```sh
cp .env.example .env
```

SQLx query metadata should be generated into `.sqlx/` and committed.

Prepare metadata with:

```sh
cargo install sqlx-cli --no-default-features --features sqlite
./scripts/prepare_sqlx_sqlite.sh
```

The script:

- creates a temporary SQLite database;
- runs `wakey-control-plane/migrations`;
- runs `cargo sqlx prepare --workspace -- -p wakey-control-plane --all-targets --all-features`;
- writes `.sqlx/`.

## Conversion Rules

Use `query!` / `query_as!` for static SQL:

- inserts;
- deletes;
- simple selects by primary key;
- fixed update statements;
- count queries.

Keep non-macro SQL for genuinely dynamic SQL:

- audit event filters built with `QueryBuilder`;
- observation listing with optional filters, unless split into fixed branches;
- any SQL where table/column names are intentionally generated.

## Test/CI

After query macros are introduced, CI should add:

```sh
cargo install sqlx-cli --no-default-features --features sqlite
./scripts/prepare_sqlx_sqlite.sh
cargo sqlx prepare --check --workspace -- --all-targets --all-features
```

Do not add the CI check until the first `.sqlx/` metadata is committed, otherwise it adds dependency install cost without catching anything.

## Notes

The SQLx docs say offline mode needs `cargo sqlx prepare` output checked into version control, and that `DATABASE_URL` takes precedence unless `SQLX_OFFLINE=true` is set. Keep `.env` local; commit `.env.example` and `.sqlx/`.
