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
