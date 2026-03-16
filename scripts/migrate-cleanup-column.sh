#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: migrate-cleanup-column.sh [--sqlite PATH] [--postgres CONNECTION]

Adds the optional `cleanup_at` column to the `words` table in an existing
SQLite or PostgreSQL database so the cleanup workflow can track when entries
have already been reviewed.

You can pass both `--sqlite` and `--postgres` to migrate both storage layers
at once. Postgres commands respect the usual environment variables such as
PGPASSWORD and PGSSLMODE when provided via the connection string.
EOF
  exit 1
}

sqlite_file=""
postgres_conn=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --sqlite)
      sqlite_file="$2"
      shift 2
      ;;
    --postgres)
      postgres_conn="$2"
      shift 2
      ;;
    --help|-h)
      usage
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      ;;
  esac
done

if [[ -z "$sqlite_file" && -z "$postgres_conn" ]]; then
  echo "Error: at least one of --sqlite or --postgres is required." >&2
  usage
fi

if [[ -n "$sqlite_file" ]]; then
  if [[ ! -f "$sqlite_file" ]]; then
    echo "SQLite file '$sqlite_file' not found." >&2
    exit 1
  fi
  if ! sqlite3 "$sqlite_file" "SELECT name FROM sqlite_master WHERE type='table' AND name='words';" | grep -q 'words'; then
    echo "SQLite file does not contain a 'words' table; run the app once to bootstrap it first." >&2
    exit 1
  fi
  if sqlite3 "$sqlite_file" "PRAGMA table_info('words');" | awk -F'|' '{print $2}' | grep -qw cleanup_at; then
    echo "SQLite columns already include cleanup_at; skipping."
  else
    sqlite3 "$sqlite_file" "ALTER TABLE words ADD COLUMN cleanup_at TEXT;"
    echo "Added cleanup_at column to SQLite database."
  fi
fi

if [[ -n "$postgres_conn" ]]; then
  if ! psql "$postgres_conn" -c "SELECT 1 FROM pg_tables WHERE tablename='words';" >/dev/null; then
    echo "Postgres database at '$postgres_conn' has no 'words' table; please initialize the schema first." >&2
    exit 1
  fi
  psql "$postgres_conn" -c "ALTER TABLE words ADD COLUMN IF NOT EXISTS cleanup_at TEXT;" >/dev/null
  echo "Ensured cleanup_at column exists on PostgreSQL."
fi
