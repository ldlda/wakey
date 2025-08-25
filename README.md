# wakey

router shit

## usage

- Build (Windows host OK):
  - cargo build --release --target armv7-unknown-linux-musleabihf
- Run on a Linux box/router:
  - copy target/armv7-unknown-linux-musleabihf/release/wakey to the device
  - chmod +x wakey && ./wakey
- Install via release asset (OpenWrt):
  - wget -O- `https://your.gitea/owner/repo/releases/download/vX.Y.Z/wakey-rootfs-vX.Y.Z-armv7-unknown-linux-musleabihf.tgz` | tar -xz -C /
  - /etc/init.d/wakey enable && /etc/init.d/wakey start

Optional updater (fetch latest automatically on OpenWrt):

```sh
WAKEY_HOST=git.ldlda.com WAKEY_OWNER=lda WAKEY_REPO=wakey sh /etc/ldlda_help/update_wakey.sh
```
