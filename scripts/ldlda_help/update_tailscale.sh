#!/bin/sh

set -e
# 0th step from lda is to move the pre assed resolv.conf to the real resolv.conf
if [ -f /tmp/resolv.conf ]; then
    cp -af /tmp/resolv.conf /etc/resolv.conf
elif [ -f /rom/etc/resolv.conf ]; then
    cp -af /rom/etc/resolv.conf /etc/resolv.conf
fi

if [ -x "$(which tailscale)" ]; then
    tailscale update
    exit 0
fi

# 1. Fetch the latest version number from Tailscale's site
LATEST=$(curl -s https://pkgs.tailscale.com/stable/ | grep -Eo 'tailscale_[0-9\.]+_arm.tgz' | head -n1)

[ -z "$LATEST" ] && {
    echo "❌ Couldn't find latest ARM binary."
    exit 1
}

URL="https://pkgs.tailscale.com/stable/${LATEST}"

cd /var/tmp

if [ -f "$LATEST" ]; then
    echo "file exists"
    rm "$LATEST"
fi

echo "Downloading $LATEST..."
wget -q "$URL"

echo "got"

# 2. Extract binaries
# workaroubd
stripped=$(basename "$LATEST" .tgz)
tar xzf "$LATEST" --strip-components=1 "$stripped/tailscale" "$stripped/tailscaled"

echo "unzipped"
# 3. Install and set permissions
chmod +x tailscale tailscaled
mv tailscale tailscaled /usr/sbin/

# 4. Restart service
/etc/init.d/tailscale restart

# 5. Cleanup
rm "$LATEST"

echo "✅ Updated to $(tailscale version)"
