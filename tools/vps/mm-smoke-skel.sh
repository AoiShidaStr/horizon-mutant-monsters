#!/bin/bash
# Headless smoke test of mutant_monsters' Mutant Skeleton on a scratch
# dedicated server (never /opt/minecraft): 127.0.0.1 only, offline mode, flat
# world, one survival bot (horizon-bots), commands over stdin, MM_TRACE=1.
S=/opt/build/mm-smoke-skel
SRV=$S/server
PORT=25594
ENGINE=${ENGINE:-/opt/build/mm-engine/horizon-engine}
MS="@e[type=mutant_monsters:mutant_skeleton,limit=1,sort=nearest]"
stop_all() {
  pkill -f "[h]orizon-bots --server 127.0.0.1:$PORT" 2>/dev/null
  pkill -f "[m]m-smoke-skel/resources" 2>/dev/null
  pkill -f "[t]ail -f /opt/build/mm-smoke-skel/console.in" 2>/dev/null
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
cmd "time set midnight"
cmd "data get entity Bot0 Health"

note "summon and attributes"
cmd "execute at Bot0 run summon mutant_monsters:mutant_skeleton ~14 ~ ~"
for i in 1 2 3 4; do sleep 1; cmd "data get entity Bot0 Health"; cmd "execute if entity @e[type=minecraft:arrow]"; done
cmd "data get entity $MS Health"
cmd "attribute $MS minecraft:max_health get"
cmd "attribute $MS minecraft:attack_damage get"
cmd "attribute $MS minecraft:follow_range get"
cmd "data get entity $MS Pos"

note "fight for 20 s, then look at its arrows"
sleep 20
cmd "execute if entity @e[type=minecraft:arrow]"
cmd "data get entity @e[type=minecraft:arrow,limit=1] NoGravity"
cmd "data get entity @e[type=minecraft:arrow,limit=1] Owner"
cmd "data get entity $MS UUID"
cmd "data get entity Bot0 Health"
cmd "data get entity Bot0 active_effects"
note "fight for 40 s more"
sleep 40
cmd "data get entity Bot0 Health"
cmd "data get entity Bot0 Pos"
cmd "data get entity $MS Pos"
cmd "data get entity $MS Health"
cmd "execute if entity @e[type=minecraft:arrow]"
cmd "effect give Bot0 minecraft:instant_health 1 20 true"

note "night healing: wounded by 10, 12 s"
cmd "damage $MS 10 minecraft:generic"
sleep 1
cmd "data get entity $MS Health"
sleep 12
cmd "data get entity $MS Health"

note "a wolf and a second skeleton beside it"
cmd "execute at $MS run summon minecraft:wolf ~1 ~ ~1"
sleep 6
cmd "execute if entity @e[type=minecraft:wolf]"
cmd "effect give Bot0 minecraft:instant_health 1 20 true"

note "death: shatter and bones"
cmd "kill @e[type=minecraft:item]"
cmd "kill @e[type=minecraft:arrow]"
cmd "tp Bot0 0 -60 0"
sleep 1
cmd "execute at Bot0 run tp $MS ~1.5 ~ ~"
cmd "data get entity Bot0 Health"
cmd "damage $MS 500 minecraft:generic"
sleep 2
cmd "data get entity Bot0 Health"
cmd "execute if entity @e[type=mutant_monsters:mutant_skeleton]"
cmd "execute if entity @e[type=minecraft:item]"
for part in limb rib shoulder_pad pelvis skull; do
  cmd "execute if entity @e[type=minecraft:item,nbt={Item:{id:\"mutant_monsters:mutant_skeleton_$part\"}}]"
done
sleep 3
cmd "clear Bot0"
cmd "kill @e[type=minecraft:item]"
cmd "effect give Bot0 minecraft:instant_health 1 20 true"

note "chemical x on two skeletons"
cmd "execute at Bot0 run summon minecraft:skeleton ~12 ~ ~ {NoAI:1b,PersistenceRequired:1b}"
cmd "execute at Bot0 run summon minecraft:skeleton ~12 ~ ~1 {NoAI:1b,PersistenceRequired:1b}"
sleep 1
cmd "execute at Bot0 run summon mutant_monsters:chemical_x ~12 ~3 ~ {Motion:[0.0d,-0.6d,0.0d]}"
sleep 10
cmd "execute if entity @e[type=minecraft:skeleton]"
cmd "execute if entity @e[type=mutant_monsters:mutant_skeleton]"
cmd "kill @e[type=mutant_monsters:mutant_skeleton]"
sleep 2
cmd "execute if entity @e[type=mutant_monsters:mutant_skeleton]"
cmd "list"
cmd "stop"
for i in $(seq 1 90); do grep -q "engine exited" $S/engine.log && break; sleep 1; done
stop_all
echo "[smoke] DONE" >> $S/engine.log
