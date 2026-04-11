#!/bin/sh
# Kill wakey by PID quietly. Prefer pidof (BusyBox), fallback to ps/awk.

# 1) Try pidof
pids="$(pidof wakey 2>/dev/null)"

# 2) Fallback: ps | awk (avoid matching awk/grep themselves)
if [ -z "$pids" ]; then
    pids="$(ps w 2>/dev/null | awk '/\/\.bin\/wakey/ && $0 !~ /awk/ {print $1}')"
fi

[ -n "$pids" ] || exit 0

# Send TERM first
kill -TERM "$pids" 2>/dev/null || true

# Optional: hard kill if still alive after a short grace
usleep 250000 # uhhh sleep is not found
remain=""
for p in $pids; do
    kill -0 "$p" 2>/dev/null && remain="$remain $p"
done
[ -z "$remain" ] || kill -KILL "$remain" 2>/dev/null || true

exit 0
