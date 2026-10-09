#!/bin/sh
# Host-side contract test; OpenWrt's UCI and procd helpers are stubbed.
set -eu
ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT HUP INT TERM

config_load() { [ "$1" = wakey ]; loaded=1; return "$load_status"; }
config_get() {
    [ "$2" = main ]
    case "$3" in
        otlp_authorization) authorization=$test_authorization ;;
        log_filter) log_filter=${test_log_filter:-$4} ;;
        *) return 1 ;;
    esac
}
procd_open_instance() { opened=1; }
procd_close_instance() { :; }
procd_set_param() {
    if [ "$1" = env ]; then
        [ "$#" -eq 2 ]
        received_filter=$2
    fi
}
procd_append_param() {
    case "$1" in
        env) [ "$#" -eq 2 ]; received_auth=$2 ;;
        file) tracked_file=$2 ;;
    esac
}
. "$ROOT/scripts/init/openwrt/wakey"
UCI_CONFIG="$TMP/wakey"
touch "$UCI_CONFIG"
load_status=0
loaded=0
opened=0
test_authorization='Basic dGVzdDp0ZXN0=='
test_log_filter='wakey_agent=debug,opentelemetry=warn'
start_service
[ "$loaded" -eq 1 ]
[ "$opened" -eq 1 ]
[ "$received_auth" = "WAKEY_OTLP_AUTHORIZATION=$test_authorization" ]
[ "$received_filter" = "RUST_LOG=$test_log_filter" ]
[ "$tracked_file" = "$UCI_CONFIG" ]

# Missing options retain the default filter and do not export empty credentials.
test_authorization=''
test_log_filter=''
received_auth=''
start_service
[ "$received_auth" = '' ]
[ "$received_filter" = 'RUST_LOG=info' ]

load_status=1
opened=0
if start_service; then
    echo 'failed UCI load was accepted' >&2
    exit 1
fi
[ "$opened" -eq 0 ]

UCI_CONFIG="$TMP/absent"
loaded=0
tracked_file=''
start_service
[ "$loaded" -eq 0 ]
[ "$tracked_file" = '' ]
[ "$received_filter" = 'RUST_LOG=info' ]
echo 'PASS: OpenWrt UCI environment forwarding and optional configuration'
