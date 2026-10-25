#!/bin/sh
# Install rsmix-cli from crates.io in a clean rust image, scaffold an app, build it and load the page.
# Run: docker run --rm -v "$PWD/bench/docker/install-check:/check" rust:1-bookworm sh /check/check.sh
# KEEP=1 (with -p 44113:44113) leaves the server up on 0.0.0.0 for browser checks from the host:
#   BASE=http://127.0.0.1:44113 TITLE="My Rsmix App" node template/e2e.mjs
set -eu
start=$(date +%s)
cargo install rsmix-cli --locked
echo "installed in $(( $(date +%s) - start )) s: $(rsmix version)"
cd /tmp
rsmix --no-color new my-rsmix-app
cd my-rsmix-app
grep '^rsmix' Cargo.toml
start=$(date +%s)
rsmix --no-color build
echo "built in $(( $(date +%s) - start )) s"
host=127.0.0.1
if [ -n "${KEEP:-}" ]; then host=0.0.0.0; fi
PORT=44113 HOST=$host ./target/release/my-rsmix-app &
server=$!
sleep 2
page=$(curl -fsS http://127.0.0.1:44113/)
echo "$page" | grep -o '<title>[^<]*</title>'
echo "$page" | grep -c 'PromptButton\|data-rmx' | sed 's/^/hydration markers: /'
entry=$(echo "$page" | grep -o 'src="/assets/[^"]*entry.js"' | head -1 | sed 's/src="//; s/"$//')
curl -fsS -o /dev/null -w "entry $entry: %{http_code}\n" "http://127.0.0.1:44113$entry"
wasm=$(find app -name '*_bg.wasm' | head -1)
curl -fsS -o /dev/null -w "wasm /assets/$wasm: %{http_code} %{size_download} bytes\n" "http://127.0.0.1:44113/assets/$wasm"
echo "install check passed"
if [ -n "${KEEP:-}" ]; then wait $server; fi
kill $server