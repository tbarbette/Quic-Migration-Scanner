#!/usr/bin/env bash
# Selects the quiche dependency source used to build migration_checker:
#   - if a local ./quiche checkout is present, build against it (path dependency)
#   - otherwise, fall back to the published crates.io release (as before)
#
# This lets you drop in a patched/forked quiche (e.g. one with multipath
# support) by cloning it into ./quiche, without touching Cargo.toml by hand.
set -euo pipefail

cd "$(dirname "$0")"

LOCAL_QUICHE_MANIFEST="quiche/quiche/Cargo.toml"
REGISTRY_VERSION="0.18.0"

if [[ -f "$LOCAL_QUICHE_MANIFEST" ]]; then
    echo "configure-quiche: using local quiche checkout at ./quiche"
    sed -i -E 's|^quiche = .*|quiche = { path = "quiche/quiche" }|' Cargo.toml
else
    echo "configure-quiche: no ./quiche checkout found, using quiche ${REGISTRY_VERSION} from crates.io"
    sed -i -E "s|^quiche = .*|quiche = \"${REGISTRY_VERSION}\"|" Cargo.toml
fi
