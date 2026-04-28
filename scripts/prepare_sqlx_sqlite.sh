#!/usr/bin/env sh
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
DB="${SQLX_PREPARE_DB:-/tmp/wakey-sqlx-prepare.sqlite3}"
DATABASE_URL="sqlite://$DB"

if ! cargo sqlx --version >/dev/null 2>&1; then
  echo "cargo-sqlx is not installed." >&2
  echo "Install it with: cargo install sqlx-cli --no-default-features --features sqlite" >&2
  exit 1
fi

rm -f "$DB" "$DB-shm" "$DB-wal"

cd "$ROOT"
DATABASE_URL="$DATABASE_URL" cargo sqlx database create
DATABASE_URL="$DATABASE_URL" cargo sqlx migrate run --source wakey-control-plane/migrations
DATABASE_URL="$DATABASE_URL" cargo sqlx prepare --workspace -- -p wakey-control-plane --all-targets --all-features
rm -rf wakey-control-plane/.sqlx
mkdir -p wakey-control-plane/.sqlx
cp .sqlx/*.json wakey-control-plane/.sqlx/

echo "SQLx metadata prepared in $ROOT/.sqlx"
echo "SQLx package metadata mirrored in $ROOT/wakey-control-plane/.sqlx"
