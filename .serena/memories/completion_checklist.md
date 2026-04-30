# Completion Checklist

Before handing back code changes, run the smallest checks covering the changed area:
- Rust formatting: `cargo fmt`.
- For control-plane SQLx/state/API work: `cargo test -p wakey-control-plane` and `SQLX_OFFLINE=true cargo test -p wakey-control-plane`.
- For SQLx query macro changes: run `scripts/prepare_sqlx_sqlite.sh` and include the `.sqlx` metadata changes.
- For wakey-linux/core inventory/observation work: `cargo test -p wakey-linux -p wakey-core`.
- For agent session/config/protocol work: `cargo test -p wakey-agent`.
- For UI work: `pnpm --dir ui typecheck` and `pnpm --dir ui build`; use `pnpm --dir ui format` when touching TS/TSX/CSS formatting.
- Run relevant `cargo clippy -p <crate> --all-targets --all-features` when behavior or API shape changes.

Always check `git status --short` and mention unrelated dirty files or generated metadata in the final summary.