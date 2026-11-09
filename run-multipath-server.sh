#!/usr/bin/env bash
# Builds and runs the local multipath-enabled QUIC test server
# (examples/multipath_server.rs) for testing `migration_checker --multipath`
# against.
#
# Usage: ./run-multipath-server.sh [listen_addr] [--no-multipath]
#   listen_addr defaults to 127.0.0.1:4433
set -euo pipefail

cd "$(dirname "$0")"

cargo build --example multipath_server
exec ./target/debug/examples/multipath_server "$@"
