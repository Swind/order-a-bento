#!/bin/sh
# Container entrypoint. With LITESTREAM_BUCKET set: restore the database from the replica when
# /data/bento.db does not exist yet, then run the server under `litestream replicate`.
# Without it: exec the server directly. Runs as the non-root `bento` user.
set -eu

DB=/data/bento.db
CONFIG=/etc/litestream.yml

if [ -n "${LITESTREAM_BUCKET:-}" ]; then
  export LITESTREAM_PATH="${LITESTREAM_PATH:-bento}"
  echo "entrypoint: Litestream enabled (bucket=${LITESTREAM_BUCKET} path=${LITESTREAM_PATH})"
  litestream restore -config "$CONFIG" -if-db-not-exists -if-replica-exists "$DB"
  # litestream supervises the server (-exec), forwards SIGTERM and exits with its status.
  exec litestream replicate -config "$CONFIG" -exec "$*"
fi

exec "$@"
