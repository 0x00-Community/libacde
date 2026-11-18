#!/usr/bin/env dash
# Run the Ime test (runtime::ime::tests in src/runtime/ime.rs, ignored by a plain cargo test) against a private fcitx5 (us keyboard + pinyin, no
# cloud pinyin) on a private D-Bus session bus, fcitx5's HOME in a fresh directory under $ACDE_CHECK_TMP (default:
# $TMPDIR, else /tmp). Builds the test first with cargo (CARGO_TARGET_DIR and the like are honoured).
# Usage: tests/runtime/ime-check.sh
set -eu
json=$(cargo test --release --locked -p libacde --lib --no-run --message-format=json)
exe=$(printf '%s\n' "$json" | grep -o '"executable":"[^"]*"' | tail -n 1 | cut -d '"' -f 4)
[ -x "$exe" ] || { echo "ime-check: no test executable" >&2; exit 1; }
home=$(mktemp -d "${ACDE_CHECK_TMP:-${TMPDIR:-/tmp}}/acde-ime.XXXXXX")
trap 'rm -rf "$home"' EXIT
mkdir -p "$home/config/fcitx5"
cat > "$home/config/fcitx5/profile" <<'PROFILE'
[Groups/0]
Name=Default
Default Layout=us
DefaultIM=pinyin

[Groups/0/Items/0]
Name=keyboard-us
Layout=

[Groups/0/Items/1]
Name=pinyin
Layout=

[GroupOrder]
0=Default
PROFILE
dbus-run-session -- sh -c '
  env -u DISPLAY -u WAYLAND_DISPLAY HOME="$1" XDG_CONFIG_HOME="$1/config" XDG_DATA_HOME="$1/data" fcitx5 --disable=cloudpinyin,xim,wayland,waylandim,classicui,kimpanel,notificationitem,xcb,ibusfrontend,fcitx4frontend,vinput -r >"$1/fcitx5.log" 2>&1 &
  fcitx=$!
  tries=0
  while [ "$tries" -lt 50 ] && ! busctl --user status org.fcitx.Fcitx5 >/dev/null 2>&1; do tries=$((tries + 1)); sleep 0.1; done
  status=0
  "$0" runtime::ime::tests --ignored --nocapture || status=$?
  # this fcitx5 only: it writes its config when it quits, before the scratch HOME goes
  kill "$fcitx" && wait "$fcitx"
  exit $status
' "$exe" "$home"
