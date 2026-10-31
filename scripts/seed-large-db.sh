#!/usr/bin/env bash
# Creates a migrated SQLite database full of synthetic rows, for measuring queries and DOM size.
# Usage: scripts/seed-large-db.sh <output.db> [photo_count=200000]
# Half of the rows keep the old `YYYY-MM-DD HH:MM:SS` format (a state only a pre-migration DB has).
# Users: alice, bob, carol, test. There are no files, so previews fail and the thumbhash shows.
set -euo pipefail
out="${1:?usage: seed-large-db.sh <output.db> [photo_count]}"
count="${2:-200000}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
rm -f "$out" "$out-shm" "$out-wal"
DATABASE_URL="sqlite:$out" sqlx database create
DATABASE_URL="sqlite:$out" sqlx migrate run --source "$root/migrations"
{ echo ".parameter set :count $count"; cat "$root/scripts/seed-large-db.sql"; } | sqlite3 "$out"
echo "seeded $out with $count photos"
