# lda-ipjs

Typed Rust wrappers around Linux `ip -j ...` output, with optional experimental rtnetlink backends.

## Purpose

`lda-ipjs` exists so `wakey` can ask Linux networking questions in typed Rust instead of:

- parsing plain-text shell output
- scattering `tokio::process::Command::new("ip")` everywhere
- mixing product logic with Linux networking trivia

## Current contract

Stable default behavior:

- `address::get(...)` uses JSON (`ip -j address show`)
- `link::get(...)` uses JSON (`ip -j link show`)
- `neighbor::get(...)` uses JSON (`ip -j neigh show`)

Optional experimental behavior:

- feature: `experimental-nl`
- enables rtnetlink-backed implementations
- intended for places where one-pass kernel queries are materially better than repeated `ip -j` calls

This means the public API is:

- `get(...)` for the default backend
- `get_with_backend(Backend::Json | Backend::Netlink)` when backend choice matters

## Modules

- `subcommands::address`
  - typed address/interface-address data
  - good place for subnet/broadcast derivation later
- `subcommands::link`
  - typed link/interface data
  - useful for `ifindex -> ifname` mapping and interface metadata
- `subcommands::neighbor`
  - typed neighbor-table data
  - currently the most useful experimental netlink surface

## Backend policy

JSON is the normal path.

Use netlink only when:

- the call is hot enough to matter
- repeated shelling out is obviously wasteful
- the netlink implementation is at least as coherent as the JSON one

Today that mainly applies to `neighbor::nl`.

## Relationship to wakey

`wakey` is the product.

`lda-ipjs` is a Linux networking adapter crate underneath it.
