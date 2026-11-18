#!/usr/bin/env bash
set -euo pipefail

here=$(cd -- "$(dirname -- "$0")" && pwd)
root=$(cd -- "$here/../.." && pwd)
state=${WPT_STATE:-$root/logs/wpt}
checkout=${WPT_DIR:-$state/checkout}
revision=$(cat "$here/revision")
export WPT_STATE=$state WPT_DIR=$checkout

fetch() {
  mkdir -p "$state"
  if [ ! -d "$checkout/.git" ]; then
    git clone --quiet --filter=blob:none --no-checkout https://github.com/web-platform-tests/wpt.git "$checkout"
  fi
  local paths=('/*')
  while IFS=$'\t' read -r dir _; do
    [ -n "$dir" ] && paths+=("!/$dir/")
  done <"$here/not-applicable"
  git -C "$checkout" sparse-checkout set --no-cone "${paths[@]}"
  if [ "$(git -C "$checkout" rev-parse HEAD 2>/dev/null || true)" != "$revision" ]; then
    git -C "$checkout" fetch --quiet --depth 1 --filter=blob:none origin "$revision"
    git -C "$checkout" -c advice.detachedHead=false checkout --quiet "$revision"
  fi
  (cd "$checkout" && ./wpt manifest --no-download -p "$state/MANIFEST.json" && ./wpt serve --help > /dev/null)
  printf 'wpt %s in %s\n' "$revision" "$checkout"
}

if [ ! -d "$here/node_modules" ]; then
  (cd "$here" && bun install --silent)
fi

command=${1:-}
[ $# -gt 0 ] && shift
case "$command" in
  fetch) fetch ;;
  prepare | run | report | compare | expectations | fyi) exec bun "$here/$command.ts" "$@" ;;
  *)
    {
      printf 'usage: scripts/wpt/wpt.sh fetch | prepare [--workers N] [--serve-port P]\n'
      printf '  | run --runtime PATH [--label L] [--workers N] [--dirs a,b] [--kinds testharness,reftest,crashtest] [--tests FILE] [--timeout-scale S]\n'
      printf '  | report LABEL | compare OLD NEW | expectations check LABEL | update LABEL… | gate | summary\n'
      printf '  | fyi [--products chrome,firefox,safari,servo] [--refresh]\n'
    } >&2
    exit 2
    ;;
esac
