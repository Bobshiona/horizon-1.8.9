#!/bin/zsh
# Build Bob Client with the release-max profile (Cargo.toml): fat LTO, one codegen
# unit, panic = abort.
#
#   tools/bob-release-max.sh [--native]
#
# --native adds -C target-cpu=native: the build then runs only on this Mac's CPU
# generation or newer, so never ship it. It builds into its own target directory
# ($CARGO_TARGET_DIR-native) because a RUSTFLAGS change rebuilds every crate.
# Prints the binary's path. Compare it with the release build through
# tools/bob-trace-bench.sh (BIN=...).
set -eu
TARGET_DIR=${CARGO_TARGET_DIR:-target}
if [[ ${1:-} == --native ]]; then
  export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=native"
  TARGET_DIR=$TARGET_DIR-native
fi
cargo build --offline --profile release-max -p horizon-189-client \
  --target aarch64-apple-darwin --target-dir "$TARGET_DIR" >&2
echo "$TARGET_DIR/aarch64-apple-darwin/release-max/horizon-189-client"
