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
#   SUDO                 Privilege escalation command/path (default: sudo when needed)
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

sudo_cmd() {
    if [ "$(id -u)" -eq 0 ]; then
        return 0
    fi

    if [ -n "${SUDO:-}" ]; then
        printf '%s' "$SUDO"
        return 0
    fi

    if command -v sudo >/dev/null 2>&1; then
        printf '%s' sudo
        return 0
    fi

    fail 'root privileges are required for install/restart steps; set SUDO or run as root'
}

priv() {
    if [ "$(id -u)" -ne 0 ]; then
        $SUDO_BIN "$@"
    else
        "$@"
    fi
}

restore_label() {
    path=$1
    if command -v restorecon >/dev/null 2>&1; then
        priv restorecon "$path" || true
    fi
}

install_executable_force() {
    src=$1
    dst=$2
    dir=$(dirname "$dst")
    tmp="$dst.new.$$"
    old="$dst.old.$$"

    priv mkdir -p "$dir"
    priv rm -f "$tmp"
    priv cp -f "$src" "$tmp"
    priv chmod 0755 "$tmp"
    if [ -e "$dst" ]; then
        priv mv -f "$dst" "$old" || true
    fi
    priv mv -f "$tmp" "$dst"
    priv rm -f "$old"
    restore_label "$dst"
}

main() {
    REPO_URL="${WAKEY_CC_REPO_URL:-https://git.ldlda.com/lda/wakey.git}"
    REF="${WAKEY_CC_REF:-main}"
    TARGET="${WAKEY_CC_TARGET:-$(default_target)}"
    ROOT="${WAKEY_CC_ROOT:-$PWD}"
    SERVICE="${WAKEY_CC_SERVICE:-wakey-cc.service}"
    SUDO_BIN=$(sudo_cmd)

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

    priv mkdir -p "$ROOT"
    install_executable_force \
        "$STAGING/bin/wakey-control-plane" \
        "$ROOT/bin/wakey-control-plane"
    priv cp -a "$STAGING/ui" "$ROOT/"
    priv cp -a "$STAGING/scripts" "$ROOT/"
    priv cp -a "$STAGING/deploy" "$ROOT/"

    if [ -z "${WAKEY_CC_NO_RESTART:-}" ] && command -v systemctl >/dev/null 2>&1; then
        if systemctl list-unit-files "$SERVICE" >/dev/null 2>&1; then
            log "restarting $SERVICE"
            priv systemctl daemon-reload
            priv systemctl restart "$SERVICE"
            priv systemctl --no-pager --full status "$SERVICE" | sed -n '1,16p'
        else
            log "service $SERVICE not installed; skipped restart"
        fi
    fi

    log "done: installed into $ROOT"
}

main "$@"
