#!/bin/sh
set -e

echo "==> Running Postgres migrations..."
rootsignal-migrate --commit

if [ "${REPLAY}" = "1" ]; then
  echo "==> Replaying events into Neo4j..."
  REPLAY=1 api
  echo "==> Replay complete, starting server..."
fi

exec api
