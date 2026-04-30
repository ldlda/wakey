# Suggested Commands

Rust formatting and checks:
- `cargo fmt`
- `cargo test -p wakey-control-plane`
- `SQLX_OFFLINE=true cargo test -p wakey-control-plane`
- `cargo test -p wakey-linux -p wakey-core`
- `cargo test -p wakey-agent` (may need elevated permissions if tests bind local sockets)
- `cargo clippy -p wakey-control-plane --all-targets --all-features`
- `cargo clippy -p wakey-linux -p wakey-core --all-targets --all-features`

SQLx metadata after query macro changes:
- `scripts/prepare_sqlx_sqlite.sh`

UI checks:
- `pnpm --dir ui typecheck`
- `pnpm --dir ui build`
- `pnpm --dir ui format`

Local run examples:
- `cargo run -- inventory <query>`
- `cargo run -- leases`
- `cargo run -- devs`
- `cargo run -- wake --mac aa:bb:cc:dd:ee:ff`
- `cargo run -p wakey-agent -- serve --config /etc/wakey-agent/config.toml`
- `cargo run -p wakey-control-plane -- serve --config-file /etc/wakey-control-plane/config.toml`

Useful shell tools:
- Prefer `rg` and `rg --files` for search.
- Use `git status --short`, `git diff --stat`, and focused `git diff -- <path>` before edits.
- Avoid destructive git commands unless explicitly requested.