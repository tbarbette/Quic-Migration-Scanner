#!/usr/bin/env bash
# Builds migration_checker and runs it with --multipath against a local
# server (e.g. one started by ./run-multipath-server.sh), then prints the
# resulting JSON.
#
# Usage: ./run-multipath-client.sh [ip] [port]
#   ip   defaults to 127.0.0.1
#   port defaults to 4433
set -euo pipefail

cd "$(dirname "$0")"

ip="${1:-127.0.0.1}"
port="${2:-4433}"

cargo build

input_file="$(mktemp)"
output_file="$(mktemp)"
trap 'rm -f "$input_file" "$output_file"' EXIT

printf 'ip,hostname,port\n%s,,%s\n' "$ip" "$port" > "$input_file"

./target/debug/migration_checker --multipath -t none --no-progress \
    -o "$output_file" "$input_file"

cat "$output_file"
