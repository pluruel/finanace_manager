#!/usr/bin/env bash
# Regenerate the sqlx offline query cache (.sqlx/) from scratch.
# Requires: postgres running on localhost:5432 with migrations applied,
#           sqlx-cli installed (`cargo install sqlx-cli`).
set -euo pipefail

cd "$(dirname "$0")"

DATABASE_URL="${DATABASE_URL:-postgres://app:app@localhost:5432/finance}"
export DATABASE_URL

echo "→ removing old .sqlx/"
rm -rf .sqlx

echo "→ running cargo sqlx prepare against $DATABASE_URL"
cargo sqlx prepare

count=$(find .sqlx -name 'query-*.json' | wc -l)
echo "✓ generated $count query files in .sqlx/"
