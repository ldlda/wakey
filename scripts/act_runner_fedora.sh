#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

RUNNER_BIN="${RUNNER_BIN:-/home/lda/gitea-runner/act_runner}"
CONFIG_PATH="${CONFIG_PATH:-$REPO_ROOT/ar_data/wsl_runner/config.yaml}"
RUNNER_HOME="${RUNNER_HOME:-$(dirname "$CONFIG_PATH")}"
SERVER_URL="${SERVER_URL:-}"
TOKEN="${TOKEN:-}"
LABELS="${LABELS:-self-hosted,linux,fedora,wsl,release:host}"

usage() {
    cat <<'EOF'
Usage:
  act_runner_fedora.sh update
  act_runner_fedora.sh register --server-url URL --token TOKEN [--labels LABELS]
  act_runner_fedora.sh start [--attach] [--once]
  act_runner_fedora.sh stop
  act_runner_fedora.sh status

Environment overrides:
    RUNNER_HOME, RUNNER_BIN, CONFIG_PATH, SERVER_URL, TOKEN, LABELS

Path overrides:
    --runner-home DIR
    --runner-bin PATH
    --config-path PATH
EOF
}

ensure_dirs() {
    mkdir -p "$RUNNER_HOME" "$(dirname "$RUNNER_BIN")"
}

ensure_runner_bin() {
    if [[ ! -x "$RUNNER_BIN" ]]; then
        echo "Runner binary not found or not executable at $RUNNER_BIN" >&2
        exit 2
    fi
}

runner_pids() {
    ps -eo pid=,args= | awk -v bin="$RUNNER_BIN" -v config="$CONFIG_PATH" '
        $2 == bin && $3 == "daemon" && $4 == "--config" && $5 == config { print $1 }
    '
}

update_runner() {
    ensure_dirs
    local api="https://gitea.com/api/v1/repos/gitea/act_runner/releases/latest"
    local release
    release="$(curl -fsSL "$api")"
    local asset_url
    asset_url="$(
        python3 - "$(printf '%s' "$release")" <<'PY'
import json, sys
data = json.loads(sys.argv[1])
for asset in data["assets"]:
    if asset["name"].endswith("linux-amd64"):
        print(asset["browser_download_url"])
        break
else:
    raise SystemExit("no linux-amd64 act_runner asset found")
PY
    )"

    local tmp="$RUNNER_HOME/act_runner.tmp"
    curl -fsSL "$asset_url" -o "$tmp"
    chmod +x "$tmp"
    mv -f "$tmp" "$RUNNER_BIN"
    echo "Updated act_runner -> $RUNNER_BIN"
}

register_runner() {
    ensure_dirs
    ensure_runner_bin

    if [[ -z "$SERVER_URL" || -z "$TOKEN" ]]; then
        echo "register requires SERVER_URL and TOKEN (or --server-url/--token)" >&2
        exit 2
    fi

    "$RUNNER_BIN" register \
        --no-interactive \
        --config "$CONFIG_PATH" \
        --instance "$SERVER_URL" \
        --token "$TOKEN" \
        --labels "$LABELS"
}

start_runner() {
    local mode="${1:-daemon}"
    ensure_dirs
    ensure_runner_bin

    if [[ ! -f "$CONFIG_PATH" ]]; then
        echo "Runner config not found at $CONFIG_PATH" >&2
        exit 2
    fi

    case "$mode" in
    attach)
        exec "$RUNNER_BIN" daemon --config "$CONFIG_PATH"
        ;;
    once)
        exec "$RUNNER_BIN" daemon --config "$CONFIG_PATH" --once
        ;;
    daemon)
        nohup "$RUNNER_BIN" daemon --config "$CONFIG_PATH" >/tmp/act_runner.log 2>&1 &
        echo "Started act_runner in background"
        ;;
    *)
        echo "Unknown start mode: $mode" >&2
        exit 2
        ;;
    esac
}

stop_runner() {
    local pids
    pids="$(runner_pids)"
    if [[ -z "$pids" ]]; then
        return 0
    fi
    kill $pids
}

status_runner() {
    ensure_runner_bin

    if [[ -n "$(runner_pids)" ]]; then
        echo "act_runner is running"
    else
        echo "act_runner is off"
    fi
}

ACTION="${1:-}"
shift || true

while [[ $# -gt 0 ]]; do
    case "$1" in
    --server-url)
        SERVER_URL="$2"
        shift 2
        ;;
    --token)
        TOKEN="$2"
        shift 2
        ;;
    --labels)
        LABELS="$2"
        shift 2
        ;;
    --runner-home)
        RUNNER_HOME="$2"
        if [[ "${RUNNER_BIN:-}" == "/home/lda/gitea-runner/act_runner" ]]; then
            RUNNER_BIN="$RUNNER_HOME/act_runner"
        fi
        if [[ "${CONFIG_PATH:-}" == "$REPO_ROOT/ar_data/wsl_runner/config.yaml" ]]; then
            CONFIG_PATH="$RUNNER_HOME/config.yaml"
        fi
        shift 2
        ;;
    --runner-bin)
        RUNNER_BIN="$2"
        shift 2
        ;;
    --config-path)
        CONFIG_PATH="$2"
        RUNNER_HOME="$(dirname "$CONFIG_PATH")"
        shift 2
        ;;
    --attach)
        START_MODE="attach"
        shift
        ;;
    --once)
        START_MODE="once"
        shift
        ;;
    *)
        echo "Unknown argument: $1" >&2
        usage
        exit 2
        ;;
    esac
done

case "$ACTION" in
update)
    update_runner
    ;;
register)
    register_runner
    ;;
start)
    start_runner "${START_MODE:-daemon}"
    ;;
stop)
    stop_runner
    ;;
status)
    status_runner
    ;;
*)
    usage
    exit 2
    ;;
esac
