#!/bin/bash
# Headless smoke test of mutant_monsters on a scratch dedicated server (never
# /opt/minecraft): 127.0.0.1 only, offline mode, flat world, one survival bot
# (horizon-bots), commands over stdin, MM_TRACE=1 for the mod's own log.
# Phase 1: attacks, lives, fire death, loot, Chemical X. Phase 2 (a restart):
# a zombie's lives survive save/load.
S=/opt/build/mm-smoke
SRV=$S/server
PORT=25593
ENGINE=${ENGINE:-/opt/build/mm-engine/horizon-engine}
MZ="@e[type=mutant_monsters:mutant_zombie,limit=1,sort=nearest]"
stop_all() {
  pkill -f "[h]orizon-bots --server 127.0.0.1:$PORT" 2>/dev/null
  pkill -f "[m]m-smoke/resources" 2>/dev/null
  pkill -f "[t]ail -f /opt/build/mm-smoke/console.in" 2>/dev/null
}
cmd() { echo "[smoke] > $*" >> $S/engine.log; echo "$*" >> $S/console.in; }
note() { echo "[smoke] ===== $* =====" >> $S/engine.log; }
start_server() {
  : > $S/console.in
  cd $SRV
  (tail -f $S/console.in | MM_TRACE=1 HORIZON_NO_AUDIO=1 $ENGINE --resources $S/resources --dedicated --nogui >> $S/engine.log 2>&1; echo "[smoke] engine exited $?" >> $S/engine.log) &
  for i in $(seq 1 300); do [ "$(grep -c 'Done (' $S/engine.log)" -ge "$1" ] && break; grep -q "engine exited" $S/engine.log && exit 1; sleep 1; done
  echo "[smoke] server up after ${i}s" >> $S/engine.log
}
start_bot() {
  /opt/build/target/debug/horizon-bots --server 127.0.0.1:$PORT --bots 1 --behaviour idle --height 0 --ground --seconds 900 --name Bot --report $S/bots.json >> $S/bots.log 2>&1 &
  for i in $(seq 1 60); do [ "$(grep -c 'Bot0 joined' $S/engine.log)" -ge "$1" ] && break; sleep 1; done
  sleep 3
  cmd "attribute Bot0 minecraft:max_health base set 1000"
  cmd "effect give Bot0 minecraft:instant_health 1 20 true"
  sleep 1
}
stop_server() {
  cmd "stop"
  for i in $(seq 1 90); do grep -q "engine exited" $S/engine.log && break; sleep 1; done
  stop_all
  sed -i 's/\[smoke\] engine exited/[smoke] engine stopped/' $S/engine.log
}

stop_all; sleep 1
mkdir -p $S
[ -d $S/resources ] || cp -r /opt/build/bomd-smoke/resources $S/resources
rm -rf $SRV; mkdir -p $SRV/horizon-mods
cp /opt/build/mods/mutant_monsters/target/mutant_monsters.hmod $SRV/horizon-mods/
echo "mod.mutant_monsters:true" > $SRV/horizon-mods.txt
echo "eula=true" > $SRV/eula.txt
cat > $SRV/server.properties <<P
server-ip=127.0.0.1
server-port=$PORT
online-mode=false
level-type=minecraft\:flat
gamemode=survival
force-gamemode=true
allow-flight=true
difficulty=normal
spawn-protection=0
pause-when-empty-seconds=0
view-distance=4
simulation-distance=4
generate-structures=false
spawn-monsters=false
spawn-animals=false
white-list=false
enforce-whitelist=false
P
: > $S/engine.log; : > $S/bots.log
start_server 1
start_bot 1
cmd "time set day"
cmd "data get entity Bot0 Health"
cmd "data get entity Bot0 Pos"

note "summon and attributes"
cmd "execute at Bot0 run summon mutant_monsters:mutant_zombie ~5 ~ ~"
sleep 2
cmd "data get entity $MZ Health"
cmd "attribute $MZ minecraft:max_health get"
cmd "attribute $MZ minecraft:attack_damage get"
cmd "attribute $MZ minecraft:knockback_resistance get"
cmd "data get entity $MZ Pos"

note "fight for 40 s"
sleep 40
cmd "data get entity Bot0 Health"
cmd "data get entity Bot0 Pos"
cmd "data get entity $MZ Pos"
cmd "execute if entity @e[type=minecraft:zombie]"
cmd "execute if entity @e[type=minecraft:husk]"
cmd "effect give Bot0 minecraft:instant_health 1 20 true"

note "lives: three falls and revivals"
for n in 1 2 3; do
  cmd "damage $MZ 500 minecraft:generic"
  sleep 1
  cmd "execute if entity @e[type=mutant_monsters:mutant_zombie]"
  cmd "damage $MZ 500 minecraft:generic"
  sleep 1
  cmd "data get entity $MZ Health"
  sleep 7
  cmd "data get entity $MZ Health"
done
note "fourth fall: death and loot"
cmd "kill @e[type=minecraft:item]"
cmd "damage $MZ 500 minecraft:generic"
sleep 3
cmd "execute if entity @e[type=mutant_monsters:mutant_zombie]"
cmd "execute if entity @e[type=minecraft:item,nbt={Item:{id:\"mutant_monsters:hulk_hammer\"}}]"
cmd "data get entity @e[type=minecraft:item,limit=1] Item"
cmd "kill @e[type=minecraft:item]"
cmd "kill @e[type=minecraft:zombie]"
cmd "kill @e[type=minecraft:husk]"
cmd "kill @e[type=minecraft:zombie_villager]"
cmd "effect give Bot0 minecraft:instant_health 1 20 true"

note "fire while down: dies at once"
cmd "execute at Bot0 run summon mutant_monsters:mutant_zombie ~6 ~ ~"
sleep 2
cmd "damage $MZ 500 minecraft:generic"
sleep 1
cmd "damage $MZ 2 minecraft:in_fire"
sleep 3
cmd "execute if entity @e[type=mutant_monsters:mutant_zombie]"
cmd "kill @e[type=minecraft:item]"

note "chemical x on three zombies"
cmd "execute at Bot0 run summon minecraft:zombie ~12 ~ ~ {NoAI:1b,PersistenceRequired:1b}"
cmd "execute at Bot0 run summon minecraft:zombie ~12 ~ ~1 {NoAI:1b,PersistenceRequired:1b}"
cmd "execute at Bot0 run summon minecraft:zombie ~13 ~ ~ {NoAI:1b,PersistenceRequired:1b}"
sleep 1
cmd "execute at Bot0 run summon mutant_monsters:chemical_x ~12 ~3 ~ {Motion:[0.0d,-0.6d,0.0d]}"
sleep 10
cmd "execute if entity @e[type=minecraft:zombie]"
cmd "execute if entity @e[type=mutant_monsters:mutant_zombie]"
cmd "kill @e[type=mutant_monsters:mutant_zombie]"
cmd "kill @e[type=minecraft:zombie]"
cmd "kill @e[type=minecraft:item]"
sleep 1

if [ -n "$CREEPER" ]; then
MC="@e[type=mutant_monsters:mutant_creeper,limit=1,sort=nearest]"
note "mutant creeper: summon and fight for 40 s"
cmd "effect give Bot0 minecraft:instant_health 1 20 true"
cmd "execute at Bot0 run summon mutant_monsters:mutant_creeper ~8 ~ ~"
sleep 2
cmd "data get entity $MC Health"
cmd "attribute $MC minecraft:max_health get"
sleep 40
cmd "data get entity Bot0 Health"
cmd "data get entity $MC Health"
cmd "data get entity $MC Pos"
note "mutant creeper: wounded, to charge"
cmd "damage $MC 40 minecraft:generic"
sleep 1
cmd "data get entity $MC Health"
sleep 20
cmd "data get entity $MC Health"
cmd "effect give Bot0 minecraft:instant_health 1 20 true"
note "mutant creeper: explosion heals it"
cmd "execute at $MC run summon minecraft:tnt ~ ~ ~ {fuse:0}"
sleep 2
cmd "data get entity $MC Health"
note "mutant creeper: death"
cmd "data get entity Bot0 Health"
cmd "damage $MC 500 minecraft:generic"
sleep 2
cmd "execute if entity @e[type=mutant_monsters:mutant_creeper]"
cmd "data get entity $MC Health"
sleep 6
cmd "execute if entity @e[type=mutant_monsters:mutant_creeper]"
cmd "execute if entity @e[type=minecraft:creeper]"
cmd "data get entity Bot0 Health"
cmd "data get entity Bot0 Pos"
cmd "effect give Bot0 minecraft:instant_health 1 20 true"
note "chemical x on two creepers"
cmd "execute at Bot0 run summon minecraft:creeper ~12 ~ ~ {NoAI:1b,PersistenceRequired:1b}"
cmd "execute at Bot0 run summon minecraft:creeper ~12 ~ ~1 {NoAI:1b,PersistenceRequired:1b}"
sleep 1
cmd "execute at Bot0 run summon mutant_monsters:chemical_x ~12 ~3 ~ {Motion:[0.0d,-0.6d,0.0d]}"
sleep 10
cmd "execute if entity @e[type=minecraft:creeper]"
cmd "execute if entity @e[type=mutant_monsters:mutant_creeper]"
cmd "kill @e[type=mutant_monsters:mutant_creeper]"
cmd "kill @e[type=minecraft:creeper]"
cmd "kill @e[type=minecraft:item]"
sleep 2
fi

note "persistence: one fall, revival, restart"
cmd "execute at Bot0 run summon mutant_monsters:mutant_zombie ~6 ~ ~ {PersistenceRequired:1b}"
sleep 2
cmd "damage $MZ 500 minecraft:generic"
sleep 9
cmd "data get entity $MZ Health"
cmd "list"
stop_server
note "restarted"
start_server 2
start_bot 2
cmd "execute if entity @e[type=mutant_monsters:mutant_zombie]"
cmd "data get entity $MZ Health"
cmd "damage $MZ 500 minecraft:generic"
sleep 2
cmd "list"
cmd "stop"
for i in $(seq 1 90); do grep -q "engine exited" $S/engine.log && break; sleep 1; done
stop_all
echo "[smoke] DONE" >> $S/engine.log
