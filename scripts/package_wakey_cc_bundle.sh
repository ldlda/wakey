#!/usr/bin/env sh
# Package wakey-control-plane bundle (binary + UI dist + updater + systemd template).
# Archive paths are relative so extraction can target any install directory.
#
# Usage:
#   ./scripts/package_wakey_cc_bundle.sh --version v0.1.0 --target x86_64-unknown-linux-gnu
#
# Optional:
#   --out-dir dist
#   --binary target/<target>/release/wakey-control-plane
#   --ui-dist ui/dist

set -eu

VERSION=""
TARGET=""
OUT_DIR="dist"
BINARY=""
UI_DIST="ui/dist"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            VERSION="$2"
            shift 2
            ;;
        --target)
            TARGET="$2"
            shift 2
            ;;
        --out-dir)
            OUT_DIR="$2"
            shift 2
            ;;
        --binary)
            BINARY="$2"
            shift 2
            ;;
        --ui-dist)
            UI_DIST="$2"
            shift 2
            ;;
        *)
            echo "unknown arg: $1" >&2
            exit 1
            ;;
    esac
done

[ -n "$VERSION" ] || { echo "--version is required" >&2; exit 1; }
[ -n "$TARGET" ] || { echo "--target is required" >&2; exit 1; }

ROOT_DIR=$(
    unset CDPATH
    cd -- "$(dirname -- "$0")/.." && pwd
)
cd "$ROOT_DIR"

if [ -z "$BINARY" ]; then
    BINARY="target/$TARGET/release/wakey-control-plane"
fi

[ -f "$BINARY" ] || { echo "missing binary: $BINARY" >&2; exit 1; }
[ -f "$UI_DIST/index.html" ] || { echo "missing UI dist index: $UI_DIST/index.html" >&2; exit 1; }
[ -f "scripts/update_wakey_cc.sh" ] && [ -f "scripts/update_wakey_cc_from_repo.sh" ] || { echo "missing updater scripts" >&2; exit 1; }
[ -f "deploy/systemd/wakey-cc.service" ] || { echo "missing systemd template" >&2; exit 1; }
[ -f "deploy/control-plane.Caddyfile" ] || { echo "missing Caddyfile" >&2; exit 1; }

mkdir -p "$OUT_DIR"
STAGING="$OUT_DIR/wakey-cc-stage.$$"
PKG="wakey-cc-$VERSION-$TARGET.tgz"

rm -rf "$STAGING"
mkdir -p "$STAGING/bin" "$STAGING/ui" "$STAGING/scripts" "$STAGING/deploy/systemd"

cp "$BINARY" "$STAGING/bin/wakey-control-plane"
cp -a "$UI_DIST" "$STAGING/ui/dist"
cp "scripts/update_wakey_cc.sh" "$STAGING/scripts/update_wakey_cc.sh"
cp "scripts/update_wakey_cc_from_repo.sh" "$STAGING/scripts/update_wakey_cc_from_repo.sh"
cp "deploy/systemd/wakey-cc.service" "$STAGING/deploy/systemd/wakey-cc.service"
cp "deploy/control-plane.Caddyfile" "$STAGING/deploy/control-plane.Caddyfile"

chmod +x "$STAGING/bin/wakey-control-plane" "$STAGING/scripts/update_wakey_cc.sh" "$STAGING/scripts/update_wakey_cc_from_repo.sh" "$STAGING/deploy/control-plane.Caddyfile"

(
    cd "$STAGING"
    tar -czf "$ROOT_DIR/$OUT_DIR/$PKG" .
)

rm -rf "$STAGING"

echo "Bundle package: $OUT_DIR/$PKG"
echo "Contains: bin/wakey-control-plane, ui/dist, scripts/update_wakey_cc.sh, scripts/update_wakey_cc_from_repo.sh, deploy/systemd/wakey-cc.service, deploy/control-plane.Caddyfile"
