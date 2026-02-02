#!/bin/sh
set -euo pipefail

APP_SUPPORT_DB="$HOME/Library/Application Support/com.languageenforcer.Language-Enforcer/words.db"
APP_SUPPORT_CONFIG="$HOME/Library/Application Support/com.languageenforcer.Language-Enforcer/config.toml"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST_DIR="$REPO_ROOT/data"

mkdir -p "$DEST_DIR"

if [ ! -f "$APP_SUPPORT_DB" ]; then
  echo "Seed DB not found: $APP_SUPPORT_DB" >&2
  exit 1
fi

cp -f "$APP_SUPPORT_DB" "$DEST_DIR/words.db"

if [ -f "$APP_SUPPORT_CONFIG" ]; then
  cp -f "$APP_SUPPORT_CONFIG" "$DEST_DIR/config.toml"
else
  echo "Config not found: $APP_SUPPORT_CONFIG (skipping)" >&2
fi

echo "Seed files exported to: $DEST_DIR"
