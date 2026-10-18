#!/usr/bin/env bash
# Project version helper. Single source of truth: backend/Cargo.toml [workspace.package].version;
# frontend/package.json and package-lock.json must follow it.
#   scripts/version.sh get            print the version
#   scripts/version.sh check          fail if the frontend version files differ
#   scripts/version.sh bump X.Y.Z     set the version everywhere (does not commit)
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cargo_toml="$root/backend/Cargo.toml"
pkg="$root/frontend/package.json"
lock="$root/frontend/package-lock.json"

get() {
  awk '
    /^\[/ { in_ws = ($0 == "[workspace.package]"); next }
    in_ws && /^version[[:space:]]*=/ { gsub(/^[^"]*"|".*$/, ""); print; found = 1; exit }
    END { exit !found }
  ' "$cargo_toml" || { echo "version.sh: no version in [workspace.package] of $cargo_toml" >&2; exit 1; }
}

check() {
  local v pv lv0 lv1 bad=0
  v=$(get)
  pv=$(jq -r .version "$pkg")
  lv0=$(jq -r .version "$lock")
  lv1=$(jq -r '.packages[""].version' "$lock")
  for pair in "frontend/package.json:$pv" "frontend/package-lock.json (version):$lv0" "frontend/package-lock.json (packages[\"\"].version):$lv1"; do
    if [ "${pair##*:}" != "$v" ]; then
      echo "version mismatch: ${pair%:*} is ${pair##*:}, backend/Cargo.toml is $v" >&2
      bad=1
    fi
  done
  if [ "$bad" = 1 ]; then
    echo "fix with: make bump V=$v   (or scripts/version.sh bump $v)" >&2
    exit 1
  fi
  echo "version $v consistent (Cargo.toml, package.json, package-lock.json)"
}

bump() {
  local new=${1:-}
  if ! [[ $new =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "usage: version.sh bump X.Y.Z (digits only, three parts)" >&2
    exit 2
  fi
  local old
  old=$(get)
  awk -v new="$new" '
    /^\[/ { in_ws = ($0 == "[workspace.package]") }
    in_ws && !done && /^version[[:space:]]*=/ { print "version = \"" new "\""; done = 1; next }
    { print }
  ' "$cargo_toml" > "$cargo_toml.tmp" && mv "$cargo_toml.tmp" "$cargo_toml"
  (cd "$root/backend" && cargo update -w)
  (cd "$root/frontend" && npm version "$new" --no-git-tag-version --allow-same-version >/dev/null)
  echo "bumped $old -> $new"
  check
  (cd "$root" && git status --short -- backend/Cargo.toml backend/Cargo.lock frontend/package.json frontend/package-lock.json 2>/dev/null || true)
}

case "${1:-}" in
  get) get ;;
  check) check ;;
  bump) shift; bump "${1:-}" ;;
  *) echo "usage: version.sh {get|check|bump X.Y.Z}" >&2; exit 2 ;;
esac
