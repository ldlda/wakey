# OpenWrt UCI for Wakey

Wakey's OpenWrt init script reads `/etc/config/wakey` for the log filter and
OTLP authorization header, then passes them to the agent through procd.
The agent's application settings remain in `/etc/wakey-agent/config.toml`.
The previous `/etc/wakey-agent/environment` file is no longer read.

## The three names

```sh
uci set wakey.main.otlp_authorization='Basic BASE64_OF_USERNAME_COLON_PASSWORD'
```

| Part | Meaning |
| --- | --- |
| `wakey` | Package: `/etc/config/wakey` |
| `main` | Named section inside that package |
| `otlp_authorization` | Option inside the section |

Quotes keep the complete `Basic ...` value together despite its space.
The credential is the Base64 encoding of `username:password`.
It is not the htpasswd hash, and Base64 is not encryption.

## Create and save

Run as root on the router. `touch` does not truncate an existing file:

```sh
touch /etc/config/wakey
chown root:root /etc/config/wakey
chmod 600 /etc/config/wakey

uci set wakey.main=service
uci set wakey.main.log_filter='info'
uci set wakey.main.otlp_authorization='Basic BASE64_OF_USERNAME_COLON_PASSWORD'
uci commit wakey
chmod 600 /etc/config/wakey
```

`wakey.main=service` creates a named section of type `service`. The type is our
chosen label, not a built-in Wakey feature. Existing options are not cleared.
`set` stages changes; `commit wakey` writes that package to disk. Committing does
not restart a service.

The resulting file looks like:

```text
config service 'main'
    option log_filter 'info'
    option otlp_authorization 'Basic BASE64_OF_USERNAME_COLON_PASSWORD'
```

The example above uses a placeholder. Avoid typing real credentials into a
command that your shell will save in history. In an interactive SSH terminal, you can
read the Basic header without echoing it and send it to UCI over stdin:

```sh
(
    set -e
    terminal_state=$(stty -g </dev/tty)
    # Restore the original terminal settings even on failure or interruption.
    trap 'stty "$terminal_state" </dev/tty' 0
    trap 'exit 1' HUP INT TERM
    printf 'Complete Basic authorization header: ' >/dev/tty
    stty -echo </dev/tty
    IFS= read -r authorization </dev/tty
    stty "$terminal_state" </dev/tty
    printf '\n' >/dev/tty
    printf "set wakey.main.otlp_authorization='%s'\ncommit wakey\n" "$authorization" | uci batch
    chmod 600 /etc/config/wakey
)
```

Use a normal `Basic <base64>` header here, without quotes/newlines inside the value.
The shell's builtin `printf` avoids passing the secret in `uci`'s command arguments.
UCI is not a secret vault: committed files, staged changes, router backups, and
root access can expose these credentials. Do not paste their output into logs/chat.

## Inspect, change, delete

Inspect the non-secret option:

```sh
uci get wakey.main.log_filter
```

`uci show wakey`, `uci export wakey`, `uci changes wakey`, and
`uci get wakey.main.otlp_authorization` **print the credential**. Use them only
when you deliberately want to see it.

Change an option:

```sh
uci set wakey.main.log_filter='wakey_agent=debug,wakey=debug,opentelemetry=warn'
uci commit wakey
```

Delete just the authorization option:

```sh
uci -q delete wakey.main.otlp_authorization
uci commit wakey
```

If TOML still requires `authorization_env`, removing the variable makes startup
fail. Remove that TOML field too only when the receiver no longer requires auth.

Discard all uncommitted changes to this package:

```sh
uci revert wakey
```

## How the service reads it

`config_load` and `config_get` are shell helpers from `/lib/functions.sh`, available
in the normal `/etc/rc.common` init-script environment. They are not standalone
commands to run directly in your SSH shell.

```sh
config_load wakey
config_get authorization main otlp_authorization
```

`config_get` arguments are **destination variable, section, option, optional default**.
The line above puts the option into the shell variable `authorization`. It does
not export it to the agent automatically; procd must receive it explicitly.

The checked-in [init script](../scripts/init/openwrt/wakey) uses these helpers.
Its configuration handling is:

```sh
start_service() {
    local authorization='' log_filter='info'
    if [ -f /etc/config/wakey ]; then
        config_load wakey || return 1
        config_get authorization main otlp_authorization
        config_get log_filter main log_filter 'info'
    fi

    procd_open_instance
    procd_set_param command "$BIN" serve --config "$CONFIG"
    procd_set_param file "$CONFIG"
    [ ! -f /etc/config/wakey ] || procd_append_param file /etc/config/wakey
    procd_set_param env "RUST_LOG=$log_filter"
    [ -z "$authorization" ] || procd_append_param env "WAKEY_OTLP_AUTHORIZATION=$authorization"
    procd_set_param respawn 5 1 0
    procd_set_param stdout 1
    procd_set_param stderr 1
    procd_close_instance
}
```

Keep these fields in the agent's TOML:

```toml
[telemetry]
otlp_endpoint = "http://10.5.0.1:4317"
authorization_env = "WAKEY_OTLP_AUTHORIZATION"
```

UCI stores the header; procd passes the environment variable; Rust reads it using
the name in TOML. The application does not parse UCI itself.

After installing the updated init script and committing settings:

```sh
/etc/init.d/wakey restart
```

Use **restart**, not the existing HUP-based reload: a running process cannot receive
changed environment variables that way. UCI is only for the router; the CC's
systemd environment file remains unchanged. See [telemetry.md](telemetry.md).

## References

- [OpenWrt UCI CLI source and command usage](https://github.com/openwrt/uci/blob/master/cli.c)
- [OpenWrt config_load/config_get helpers](https://github.com/openwrt/openwrt/blob/main/package/base-files/files/lib/functions.sh)
