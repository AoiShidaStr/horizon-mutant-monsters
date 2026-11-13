#!/bin/bash
# mutant_monsters: stage the crate, run its tests, build the dedicated-server
# engine (no default features) and keep a copy, then check the client flavour.
. ~/.cargo/env
export CARGO_TARGET_DIR=/opt/build/target CARGO_BUILD_JOBS=4
cd /opt/build/wt/mutant_monsters
/opt/build/mm-stage.sh
date
echo "== mod tests"
nice -n 10 cargo test -p mod-mutant_monsters --message-format short 2>&1 | grep -vE "^\s+(Compiling|Checking)" | grep -E "test result|FAILED|panicked|error|warning|-->" ; echo "test_exit=${PIPESTATUS[0]}"
if [ "$1" != client ]; then
echo "== server engine (no default features)"; date
nice -n 10 cargo build -p horizon-native-game --no-default-features --message-format short 2>&1 | grep -vE "^\s+(Compiling|Checking)"; echo "build_exit=${PIPESTATUS[0]}"
mkdir -p /opt/build/mm-engine && cp /opt/build/target/debug/horizon-client ${ENGINE_OUT:-/opt/build/mm-engine/horizon-engine} && grep -c "mutant_monsters:mutant_creeper" ${ENGINE_OUT:-/opt/build/mm-engine/horizon-engine}; ls -la /opt/build/mm-engine
fi
echo "== client flavour check (default features)"; date
nice -n 10 cargo check -p horizon-native-game --message-format short 2>&1 | grep -vE "^\s+(Compiling|Checking)"; echo "client_exit=${PIPESTATUS[0]}"
date; echo DONE
