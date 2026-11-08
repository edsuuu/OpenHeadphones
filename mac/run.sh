#!/bin/sh
# Builds and runs the macOS app. Needs only the Command Line Tools (swiftc).
# Arguments go to the app: --open (show the panel right away), --debug (log every packet),
# --selftest (check the protocol and the data files), --snapshot <dir> (every screen as PNGs).
# Bluetooth permission belongs to the app that starts this one (the terminal).
set -e
cd "$(dirname "$0")"
mkdir -p .build
swiftc -O -swift-version 5 *.swift -o .build/jbl-headphones
exec .build/jbl-headphones "$@"
