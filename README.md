# flick-sync

<p align="center">
  <img width="100" height="100" src="logo/logo.svg">
</p>

Tools to support automatically synchronising media from a [Plex Media Server](https://www.plex.tv/) to a local disk.

Currently includes the following:

* [`flick-sync`](packages/flick-sync). A Rust crate to manage and update a set of synced items.
* [`flick-sync-cli`](packages/cli). A command line tool to perform the syncing.
* [`dlna-server`](packages/dlna-server). A rust crate implementing a basic DLNA media server.
* [`mobile`](packages/mobile). A React Native mobile app to play back synchronised content.

## OpenWrt

The server can be built as opkg packages for aarch64 OpenWrt devices such as
the GL.iNet GL-BE10000 with `mise run package:openwrt`. This cross-compiles a
static binary and writes two packages to `target/openwrt`:

* `flick-sync`, the server with a procd init script and UCI config.
* `luci-app-flick-sync`, a LuCI page under Services for configuring it.

Install them through LuCI or with `opkg install`. By default the service looks
for a store on mounted external storage by checking the root of each partition
under `/tmp/mountd`, and each directory one level below it, for a
`flicksync.json` file. A store directory can instead be set explicitly from the
LuCI page or in `/etc/config/flick-sync`. When the storage holding the store is
removed the service restarts and searches again.
