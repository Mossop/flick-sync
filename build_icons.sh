#! /bin/sh

TARGET="packages/cli/resources/upnp"

resvg logo/logo.svg $TARGET/logo-32.png --width 32 --height 32
resvg logo/logo.svg $TARGET/logo-64.png --width 64 --height 64
resvg logo/logo.svg $TARGET/logo-128.png --width 128 --height 128
resvg logo/logo.svg $TARGET/logo-256.png --width 256 --height 256

ASSETS="packages/cli/resources/shoelace/assets/icons"
RESVG_OPTIONS="--width 180 --height 180"
IM_OPTIONS="-background transparent -gravity center -extent 256x256"

resvg $RESVG_OPTIONS $ASSETS/tv.svg $TARGET/television-256.png
magick $TARGET/television-256.png $IM_OPTIONS $TARGET/television-256.png

resvg $RESVG_OPTIONS $ASSETS/film.svg $TARGET/movie-256.png
magick $TARGET/movie-256.png $IM_OPTIONS $TARGET/movie-256.png

resvg $RESVG_OPTIONS $ASSETS/collection.svg $TARGET/library-256.png
magick $TARGET/library-256.png $IM_OPTIONS $TARGET/library-256.png

resvg $RESVG_OPTIONS $ASSETS/music-note-list.svg $TARGET/media-256.png
magick $TARGET/media-256.png $IM_OPTIONS $TARGET/media-256.png
