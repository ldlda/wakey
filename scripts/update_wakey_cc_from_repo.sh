#!/usr/bin/env sh
# Build and install wakey-control-plane from repository source on the target host.
# This avoids glibc baseline mismatches from prebuilt artifacts by building locally,
# then packaging the same bundle format and installing it.
#
# Simplest use:
#   sudo -E ./scripts/update_wakey_cc_from_repo.sh
#
# Env options:
#   WAKEY_CC_REPO_URL    Git URL (default: https://git.ldlda.com/lda/wakey.git)
#   WAKEY_CC_REF         Git ref/branch/tag (default: main)
#   WAKEY_CC_TARGET      Rust target triple (default from uname)
#   WAKEY_CC_ROOT        Install root (default: current directory)
#   WAKEY_CC_SERVICE     systemd unit name (default: wakey-cc.service)
#   WAKEY_CC_VERSION     Bundle version label (default: v0.0.0-local-<short_sha>)
#   WAKEY_CC_NO_RESTART  If set, skip systemd restart
#   WAKEY_CC_KEEP_TMP    If set, keep temp checkout/build dir for debugging
#
# Requires: git, cargo, tar, pnpm, systemctl (optional for restart)

set -eu

log() { printf '[update_wakey_cc_from_repo] %s\n' "$*"; }
fail() {
    printf '[update_wakey_cc_from_repo] ERROR: %s\n' "$*" >&2
    exit 1
}

default_target() {
    arch=$(uname -m)
    case "$arch" in
        x86_64)
            printf '%s' 'x86_64-unknown-linux-gnu'
            ;;
        aarch64|arm64)
            printf '%s' 'aarch64-unknown-linux-gnu'
            ;;
        *)
            fail "unsupported arch for default target mapping: $arch (set WAKEY_CC_TARGET explicitly)"
            ;;
    esac
}

require_cmd() {
    command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

main() {
    REPO_URL="${WAKEY_CC_REPO_URL:-https://git.ldlda.com/lda/wakey.git}"
    REF="${WAKEY_CC_REF:-main}"
    TARGET="${WAKEY_CC_TARGET:-$(default_target)}"
    ROOT="${WAKEY_CC_ROOT:-$PWD}"
    SERVICE="${WAKEY_CC_SERVICE:-wakey-cc.service}"

    require_cmd git
    require_cmd cargo
    require_cmd tar
    require_cmd pnpm

    TMPBASE="${TMPDIR:-/tmp}"
    WORKDIR="$TMPBASE/wakey-cc-src.$$"
    STAGING="$TMPBASE/wakey-cc-stage.$$"

    cleanup() {
        if [ -z "${WAKEY_CC_KEEP_TMP:-}" ]; then
            rm -rf "$WORKDIR" "$STAGING"
        else
            log "keeping temp dir: $WORKDIR"
        fi
    }
    trap cleanup EXIT INT TERM

    log "cloning $REPO_URL (ref: $REF)"
    git clone --depth 1 --branch "$REF" "$REPO_URL" "$WORKDIR" ||
        fail 'git clone failed'

    cd "$WORKDIR"

    log 'building UI dist'
    (
        cd ui
        pnpm install --frozen-lockfile
        pnpm build
    )

    log "building control-plane target $TARGET"
    cargo build --release --target "$TARGET" -p wakey-control-plane ||
        fail 'cargo build failed'

    SHA=$(git rev-parse --short HEAD)
    VERSION="${WAKEY_CC_VERSION:-v0.0.0-local-$SHA}"

    log "packaging bundle version $VERSION"
    ./scripts/package_wakey_cc_bundle.sh --version "$VERSION" --target "$TARGET" ||
        fail 'bundle packaging failed'

    PKG="dist/wakey-cc-$VERSION-$TARGET.tgz"
    [ -f "$PKG" ] || fail "bundle not found: $PKG"

    rm -rf "$STAGING"
    mkdir -p "$STAGING"
    tar -xzf "$PKG" -C "$STAGING" || fail 'bundle extract failed'

    [ -f "$STAGING/bin/wakey-control-plane" ] || fail 'bundle missing bin/wakey-control-plane'
    [ -f "$STAGING/ui/dist/index.html" ] || fail 'bundle missing ui/dist/index.html'

    mkdir -p "$ROOT"
    cp -a "$STAGING/." "$ROOT/"

    if [ -z "${WAKEY_CC_NO_RESTART:-}" ] && command -v systemctl >/dev/null 2>&1; then
        if systemctl list-unit-files "$SERVICE" >/dev/null 2>&1; then
            log "restarting $SERVICE"
            systemctl daemon-reload
            systemctl restart "$SERVICE"
            systemctl --no-pager --full status "$SERVICE" | sed -n '1,16p'
        else
            log "service $SERVICE not installed; skipped restart"
        fi
    fi

    log "done: installed into $ROOT"
}

main "$@"
