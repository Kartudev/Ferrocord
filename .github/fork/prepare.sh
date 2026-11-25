#!/usr/bin/env bash
# Fork only. Run inside the checked-out source tree:
#   prepare.sh <patch> <version>
# Applies the FERRO_UPDATE_REPO patch when the tree lacks it, then sets the
# test version in Cargo.toml + Cargo.lock.
set -euo pipefail
patch="$1" version="$2"
if ! grep -q FERRO_UPDATE_REPO src/update/mod.rs; then
  git apply --3way "$patch" \
    || { echo "::error::update-repo-override.patch no longer applies; refresh it"; exit 1; }
fi
grep -q FERRO_UPDATE_REPO src/update/mod.rs
sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
sed -i "/^name = \"ferrocord\"\r\?$/{n;s/^version = \".*\"/version = \"$version\"/}" Cargo.lock
grep -m1 '^version = ' Cargo.toml
