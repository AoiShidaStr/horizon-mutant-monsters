#!/bin/bash
# Type-check mutant_monsters: the mod crate (server flavour, no default features), its tests,
# and the client flavour (horizon-native-game with default features).
. ~/.cargo/env
export CARGO_TARGET_DIR=/opt/build/target CARGO_BUILD_JOBS=4
cd /opt/build/wt/mutant_monsters
/opt/build/mm-stage.sh
MODE=${1:-check}
echo "== mod crate check (server flavour)"; date
nice -n 10 cargo check -p mod-mutant_monsters --message-format short 2>&1 | grep -vE "^\s+(Compiling|Checking)"
echo "check_exit=${PIPESTATUS[0]}"
if [ "$MODE" = full ]; then
  echo "== mod tests"; date
  nice -n 10 cargo test -p mod-mutant_monsters --message-format short 2>&1 | grep -vE "^\s+(Compiling|Checking)" | tail -40
  echo "test_exit=${PIPESTATUS[0]}"
  echo "== client flavour check (default features)"; date
  nice -n 10 cargo check -p horizon-native-game --message-format short 2>&1 | grep -vE "^\s+(Compiling|Checking)"
  echo "client_exit=${PIPESTATUS[0]}"
  echo "== server flavour check (no default features)"; date
  nice -n 10 cargo check -p horizon-native-game --no-default-features --message-format short 2>&1 | grep -vE "^\s+(Compiling|Checking)"
  echo "server_exit=${PIPESTATUS[0]}"
fi
date; echo DONE
