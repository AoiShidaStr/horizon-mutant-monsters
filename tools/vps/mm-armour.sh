#!/bin/bash
# Headless check of the new Mutant Skeleton armour set on a scratch dedicated server.
S=/opt/build/mm-armour
SRV=$S/server
PORT=25594
ENGINE=${ENGINE:-/opt/build/mm-engine/horizon-engine}
stop_all() {
  pkill -f "[h]orizon-bots --server 127.0.0.1:$PORT" 2>/dev/null
  pkill -f "[m]m-armour/resources" 2>/dev/null
  pkill -f "[t]ail -f $S/console.in" 2>/dev/null
}
cmd() { echo "[armour] > $*" >> $S/engine.log; echo "$*" >> $S/console.in; }
note() { echo "[armour] ===== $* =====" >> $S/engine.log; }
stop_all; sleep 1
mkdir -p $S
[ -d $S/resources ] || cp -r /opt/build/bomd-smoke/resources $S/resources
rm -rf $SRV; mkdir -p $SRV/horizon-mods
cp /tmp/mm.hmod $SRV/horizon-mods/mutant_monsters.hmod
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
: > $S/console.in
cd $SRV
(tail -f $S/console.in | MM_TRACE=1 HORIZON_NO_AUDIO=1 $ENGINE --resources $S/resources --dedicated --nogui >> $S/engine.log 2>&1; echo "[armour] engine exited" >> $S/engine.log) &
for i in $(seq 1 300); do [ "$(grep -c "Done (" $S/engine.log)" -ge 1 ] && break; grep -q "engine exited" $S/engine.log && break; sleep 1; done
echo "[armour] server up after ${i}s" >> $S/engine.log
/opt/build/target/debug/horizon-bots --server 127.0.0.1:$PORT --bots 1 --behaviour idle --height 0 --ground --seconds 300 --name Bot --report $S/bots.json >> $S/bots.log 2>&1 &
for i in $(seq 1 60); do [ "$(grep -c "Bot0 joined" $S/engine.log)" -ge 1 ] && break; sleep 1; done
sleep 3
note "register the items"
for it in mutant_skeleton_skull mutant_skeleton_chestplate mutant_skeleton_leggings mutant_skeleton_boots mutant_skeleton_arms mutant_skeleton_rib_cage; do
  cmd "give Bot0 mutant_monsters:$it"
done
cmd "give Bot0 mutant_monsters:mutant_skeleton_limb 8"
cmd "give Bot0 mutant_monsters:mutant_skeleton_rib 6"
cmd "give Bot0 mutant_monsters:mutant_skeleton_shoulder_pad 2"
cmd "give Bot0 mutant_monsters:mutant_skeleton_pelvis"
sleep 2
note "the recipes exist"
for r in mutant_skeleton_arms mutant_skeleton_rib_cage mutant_skeleton_chestplate mutant_skeleton_leggings mutant_skeleton_boots; do
  cmd "recipe give Bot0 mutant_monsters:$r"
done
sleep 2
note "wear the set and read the attributes"
cmd "item replace entity Bot0 armor.head with mutant_monsters:mutant_skeleton_skull"
cmd "item replace entity Bot0 armor.chest with mutant_monsters:mutant_skeleton_chestplate"
cmd "item replace entity Bot0 armor.legs with mutant_monsters:mutant_skeleton_leggings"
cmd "item replace entity Bot0 armor.feet with mutant_monsters:mutant_skeleton_boots"
sleep 2
cmd "attribute Bot0 minecraft:armor get"
cmd "attribute Bot0 minecraft:armor_toughness get"
cmd "attribute Bot0 minecraft:movement_speed get"
cmd "attribute Bot0 minecraft:step_height get"
cmd "attribute Bot0 minecraft:jump_strength get"
cmd "attribute Bot0 minecraft:safe_fall_distance get"
sleep 3
note "stop"
cmd "stop"
for i in $(seq 1 90); do grep -q "engine exited" $S/engine.log && break; sleep 1; done
stop_all
sed -i "s/\[armour\] engine exited/[armour] engine stopped/" $S/engine.log
echo DONE
