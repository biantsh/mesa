#!/bin/sh
set -e
variables=$(node "$(dirname "$0")/cargo.mjs" "$@")
while IFS= read -r line; do
  [ -n "$line" ] && export "$line"
done <<EOF_VARIABLES
$variables
EOF_VARIABLES
exec "${CARGO:-cargo}" "$@"
