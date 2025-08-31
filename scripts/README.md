# Scripts

This folder has small helpers for build, CI, and router install. Keep it simple; use only what you need.

## Quick start (recommended)

1. Start your Gitea runner on this Windows PC (if not already running):

   ```powershell
   ./scripts/act_runner.ps1
   ```

   If first-time, it prints the exact register command. Run it once, check labels include `windows:host,self-hosted`, then rerun the script.

2. Publish, tag, and push (recommended):

   ```powershell
   ./scripts/publish.ps1 -Tag
   ```

   This will bump the version (if you specify one), build, ensure the runner is started, tag, and push. To also publish to the registry, add `-Publish`.

3. Install on the router

   Use the updater on the router to fetch the latest automatically:

   ```sh
   WAKEY_HOST=git.ldlda.com WAKEY_OWNER=lda WAKEY_REPO=wakey sh /etc/ldlda_help/update_wakey.sh
   ```

   Alternatively, use the release asset URL from your Gitea Release page:

   ```sh
   wget -O- https://<gitea>/<owner>/<repo>/releases/download/v0.1.0/wakey-rootfs-v0.1.0-armv7-unknown-linux-musleabihf.tgz | tar -xz -C /
   chmod +x /etc/init.d/wakey && /etc/init.d/wakey enable && /etc/init.d/wakey restart
   ```

## Scripts overview

- `act_runner.ps1` — Start/seed the local Gitea runner. Use `-Attach` to see logs, `-ForceConfigure` to register non-interactively.
- `dev_push.ps1` — Fast dev loop: build + upload to router. Usage:
  - `./scripts/dev_push.ps1 -Pass <password> [-HostName <ip>] [-RemotePath </root/.bin/wakey>] [-Restart] [-Quiet]`
  - Uploads to `<RemotePath>.tmp` then atomically moves into place; `-Restart` restarts the service; `-Quiet` silences MOTD.
- `package_rootfs.ps1` — Produces `dist/wakey-rootfs-<version>-<target>.tgz` with `/root/.bin/wakey` and `/etc/init.d/*`.
- `publish.ps1` — Optional version bump + build + tag (and `cargo publish` only if you pass `-Publish`).

## CI (Gitea)

- Defined in `.gitea/workflows/release.yml`.
- Runs on your `self-hosted, windows` runner.
- Steps: cross build → package rootfs → upload artifact (v3) → publish a release and attach tgz.
- Requires `secrets.GITEA_TOKEN` in the repo to publish the release.

## Local build/install (manual path)

```powershell
# Build
cargo build --release --target armv7-unknown-linux-musleabihf

# Package rootfs
./scripts/package_rootfs.ps1 -Version v0.1.0 -Target armv7-unknown-linux-musleabihf

# Copy to router (PowerShell)
scp -O .\dist\wakey-rootfs-v0.1.0-armv7-unknown-linux-musleabihf.tgz root@<router-ip>:/tmp/wakey.tgz
```

```sh
# On the router
tar -xz -f /tmp/wakey.tgz -C /
chmod +x /etc/init.d/wakey && /etc/init.d/wakey enable && /etc/init.d/wakey restart
```

Or you can use the script:

```powershell
./scripts/dev_push.ps1 -Pass <password>
```

That’s it. Keep the flow: start runner → tag push → install.
