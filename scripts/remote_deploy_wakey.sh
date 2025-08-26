#!/bin/sh
# Usage: remote_deploy_wakey.sh <bin_tmp> <dest_path> [restart_flag]
# restart_flag: 1 (default) to stop/start, 0 to only replace file.

set -e
BIN_TMP="$1"
DEST="$2"
RESTART="${3:-1}"
INIT="/etc/init.d/wakey"
KILL="/root/.bin/kill_wakey.sh"

if [ -z "$BIN_TMP" ] || [ -z "$DEST" ]; then
  echo "usage: $0 <bin_tmp> <dest_path> [restart_flag]" >&2
  exit 2
fi

[ -f "$BIN_TMP" ] || { echo "tmp binary not found: $BIN_TMP" >&2; exit 1; }
chmod +x "$BIN_TMP" || true

if [ "$RESTART" = "1" ]; then
  if [ -x "$INIT" ]; then
    "$INIT" stop || true
  elif [ -x "$KILL" ]; then
    sh "$KILL" || true
  fi
fi

mv -f "$BIN_TMP" "$DEST"

if [ "$RESTART" = "1" ]; then
  if [ -x "$INIT" ]; then
    "$INIT" start || "$INIT" restart || true
  else
    nohup "$DEST" >/dev/null 2>&1 &
  fi
fi

exit 0
