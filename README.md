# flick-sync

<p align="center">
  <img width="100" height="100" src="logo/logo.svg">
</p>

flick-sync keeps a local copy of selected media from one or more
[Plex Media Servers](https://www.plex.tv/) so that it can be watched without a
connection to the server, for example while travelling. It downloads (and where
needed transcodes) the movies, shows and playlists you choose, keeps the copy up
to date, and serves it to the devices around it through a web interface, DLNA
and Chromecast. Watch progress is kept in step with Plex.

It runs anywhere Rust does, including small devices such as a Raspberry Pi or a
travel router running OpenWrt.

## Features

- **Choose what to sync** by pasting the item's URL from the Plex web app.
  Movies, shows, seasons, playlists and movie or show collections can be synced,
  from any number of servers.
- **Transcoding profiles** control the quality of downloads. `720p` (the
  default) and `1080p` are built in, and you can define your own. A profile can
  be set per server or per synced item; when an item is included by more than
  one sync, the highest quality profile wins.
- **Only unplayed** syncs keep just the episodes or videos you haven't watched,
  so a show's next few episodes are always available without storing the whole
  series.
- **Two-way watch progress.** Progress and watched state from playback on the
  local copy are sent back to Plex on the next sync, and changes made in Plex
  are picked up, unless the item has been watched on the server since.
- **Resilient downloads.** Downloads run in parallel (4 at a time by default)
  and interrupted downloads resume where they left off.
- **Web interface** to browse libraries, collections, shows and playlists, play
  videos in the browser, pick up where you left off, manage what is synced and
  follow sync progress.
- **Chromecast support** from the web interface, including cast devices on a
  different network to the server.
- **DLNA media server** so smart TVs, game consoles and other DLNA players can
  browse and play the synced media.
- **Background syncing** every 30 minutes while the server is running, or on
  demand from the web interface.
- **Optional HTTPS**, with certificates reloaded automatically when they are
  renewed.
- **Standardized output** can lay files out with `.nfo` metadata, cover images
  and `.m3u` playlists in folders named after libraries, shows and movies, for
  use with other media players.
- **Android app** to play synced content, either from local storage or
  streamed from a flick-sync server on the network, which it finds
  automatically through the server's DLNA announcements.
- **OpenWrt packages** with a LuCI configuration page, see [below](#openwrt).

## Getting started

Build the command line tool with a recent stable Rust toolchain:

```bash
cargo build --release
```

This produces `target/release/flick-sync`. Cross-compiling for a Raspberry Pi
or OpenWrt is covered by the [`mise`](https://mise.jdx.dev/) tasks in
`mise.toml`, e.g. `mise run build:pi4`.

Everything flick-sync stores lives in a single directory, the *store*. Pass it
with `--store` or set `FLICK_SYNC_STORE`. A new store must be an empty
directory.

```bash
export FLICK_SYNC_STORE=/media/flick-sync
```

Log in to a Plex server, giving it a short identifier of your choosing. You can
sign in with a Plex account (including managed users, with their PIN) or
connect directly to a server's address:

```bash
flick-sync login home
```

Add the things to sync by copying their URL from the Plex web app:

```bash
flick-sync add 'https://app.plex.tv/desktop/#!/server/…/details?key=%2Flibrary%2Fmetadata%2F12345'
```

Then download everything:

```bash
flick-sync sync
```

And serve it on port 3000:

```bash
flick-sync serve --port 3000
```

## Commands

| Command | Description |
|---|---|
| `login <id>` | Log in, or log in again, to a Plex server. `--transcode-profile` sets the server's default profile. |
| `add <url>` | Add an item to sync. `--profile` picks a transcode profile, `--only-unplayed` only keeps unwatched videos. |
| `remove <server> <id>` | Stop syncing an item. |
| `list` | List the synced items. |
| `sync` | Sync everything, or just the servers given with `--server`. |
| `prune` | Update the lists of items to sync and delete local files that are no longer needed. |
| `serve` | Run the web interface and DLNA server, syncing in the background. |
| `stats` | Show download statistics. |
| `set-output-style <minimal\|standardized>` | Choose how files are laid out in the store. |
| `build-metadata` | Rebuild metadata files. |
| `recover` | Attempt to recover from a corrupt state file. |

Run `flick-sync help <command>` for all of a command's options.

### Serving

`flick-sync serve` runs the web interface, a DLNA server and the background
sync.

| Option | Environment variable | Description |
|---|---|---|
| `--port` | `FLICK_SYNC_PORT` | Port for the web interface, 80 by default. |
| `--certificate` | `FLICK_SYNC_CERTIFICATE` | TLS certificate (PEM or DER, e.g. a `.cer` file) to also serve over HTTPS. |
| `--private-key` | `FLICK_SYNC_PRIVATE_KEY` | Private key for the certificate (PEM or DER, e.g. a `.key` file). |
| `--https-port` | `FLICK_SYNC_HTTPS_PORT` | Port for HTTPS, 443 by default. |
| `--disable-syncing` | `FLICK_SYNC_DISABLE_SYNCING` | Serve the existing content without syncing from Plex. |

The certificate and key files are checked for changes every 30 seconds, so a
renewed certificate is picked up without a restart.

### Logging

`--log-level` (or `FLICK_SYNC_LOG_LEVEL`) limits logging to `error`, `warn`,
`info`, `debug` or `trace` and above. Log output is coloured and spread over
several lines on a terminal, and kept to one plain line per message otherwise.
Set `NO_COLOR` to turn off colours.

## Configuration

Most settings are managed with the commands above, but some are only available
by editing `flicksync.json` in the store:

```json
{
  "maxDownloads": 4,
  "profiles": {
    "phone": {
      "bitrate": 1500,
      "dimensions": [854, 480]
    }
  }
}
```

- `maxDownloads` is the number of downloads to run at once.
- `profiles` adds transcode profiles. Each can set `bitrate` (kbps),
  `dimensions` (`[width, height]`), `containers`, `video_codecs`,
  `audio_codecs`, `audio_channels`, `h264_profiles` and `h264_level`. Anything
  not set falls back to 2000 kbps, 1280×720, MP4, H.264 and AAC or MP3.

## OpenWrt

The server can be built as opkg packages for aarch64 OpenWrt devices such as
the GL.iNet GL-BE10000 with `mise run package:openwrt`. This cross-compiles a
static binary and writes two packages to `target/openwrt`:

- `flick-sync`, the server with a procd init script and UCI config.
- `luci-app-flick-sync`, a LuCI page under Services for configuring it.

Install them through LuCI or with `opkg install`. Everything below can be set
from the LuCI page or in `/etc/config/flick-sync`.

- **Store:** by default the service looks for a store on mounted external
  storage, checking the root of each partition under `/tmp/mountd`, and each
  directory one level below it, for a `flicksync.json` file. A store directory
  can be set explicitly instead. When the storage holding the store is removed
  the service restarts and searches again.
- **Syncing** is off by default, so the router serves what is already in the
  store. Turn on "Sync from Plex" to keep it up to date.
- **Ports:** the web interface uses port 3000, and HTTPS (when a certificate
  and key are set) port 3443, since LuCI uses 80 and 443.
- **Cast devices:** while the server runs, the firewall accepts mDNS and the
  web interface port from the LAN, or from both the LAN and WAN. Traffic from
  the WAN is only accepted from private addresses. This allows casting to
  devices on an upstream network, such as a hotel's.
- **Hostname:** setting an unused LAN address makes the web interface
  reachable as `http://media.lan/` (the name is configurable). The address is
  added to the router while the server runs, the name is published through
  dnsmasq, and ports 80 and 443 on it are forwarded to the web interface.
- **Logging** goes to the system log, at warnings and errors only by default.

## Project layout

- [`flick-sync`](packages/flick-sync): a Rust crate that manages and updates a
  store of synced items.
- [`flick-sync-cli`](packages/cli): the `flick-sync` command line tool,
  including the web interface.
- [`dlna-server`](packages/dlna-server): a Rust crate implementing a basic DLNA
  media server.
- [`mobile`](packages/mobile): a React Native app for playing synced content on
  Android, from local storage or a flick-sync server.
- [`openwrt`](openwrt): OpenWrt packaging and the LuCI app.
