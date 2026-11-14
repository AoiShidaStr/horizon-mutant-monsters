# Mutant Monsters for Horizon

A Horizon port of [Mutant Monsters](https://github.com/Fuzss/mutantmonsters)
by shcott21, Chumbanotz, Fuzs and tdstress (AGPL-3.0). For Saison 2 it takes
the place of Mowzie's Mobs. This slice adds the **Mutant Zombie**, the
**Mutant Creeper** and the **Mutant Skeleton** bosses, the zombie's
**Hulk Hammer** drop, the skeleton's bones and the **Chemical X** splash that
turns a zombie, a creeper or a skeleton into its mutant.

> **Source code (AGPL-3.0, section 13).** This server runs modified AGPL code.
> Its complete source is in the `mutant_monsters.hnmod` file (a zip) and in the
> `mods/mutant_monsters/` folder of the Saison 2 server repository. The
> original is at https://github.com/Fuzss/mutantmonsters (commit `0a8e21bd`).
> Ask an admin for a copy of either.

- Content mod `.hmod` (`side = "both"`): the `mutant_zombie`, `mutant_creeper`,
  `mutant_skeleton` and `chemical_x` entity types, the `hulk_hammer` and
  `chemical_x` items, the five Mutant Skeleton bone items and the four-piece
  Mutant Skeleton armour set, a creative tab,
  the loot tables, damage types, tags, a Chemical X recipe, en_us/fr_fr names
  and original placeholder textures.
- Native trait mod `.hnmod`: the Mutant Zombie's model and animations, its
  goals, slam/throw/roar attacks, seismic waves, zombie resurrection, three
  lives, the hammer's waves and the Chemical X conversion; the Mutant
  Creeper's model, poses, goals, jump and charge attacks and its death blast;
  the Mutant Skeleton's model, poses, goals, swing, rib crush, shot,
  multi-shot leap and its shattering death.

The Java code was translated to Rust (see `NOTICE.md` for what comes from
where). Upstream's textures and sounds are All Rights Reserved and are not
used: the textures are this port's own, the sounds are vanilla ones.

## Install

Both halves use the id `mutant_monsters`.

1. Put `mutant_monsters.hmod` in `horizon-mods/` and `mutant_monsters.hnmod`
   in `horizon-native-mods/`, and turn them on in `horizon-mods.txt`
   (`mod.mutant_monsters:true`).
2. Build the native half into the game (launcher **Mods** page, or
   `horizon-launcher native-build <folder>`; the launcher asks the operator
   to trust native code). A dedicated server needs a server engine built with
   the mod: `horizon-launcher native-build <folder> --server-output <dir>`.
3. Clients and the dedicated server need the same `.hmod` (a `side = "both"`
   mod; a client with another version is refused) and clients need the native
   half too, or the zombie is drawn as a plain zombie and the throw doesn't
   push them.

Natural spawning is **off** in this build. `python tools/pack.py --spawns`
adds biome modifiers (overworld, upstream's weights) once the mobs have been
tried with real players; natural spawning itself was not tested. Without it, a
mutant comes from `/summon mutant_monsters:mutant_zombie` (or
`mutant_creeper`, `mutant_skeleton`) or a Chemical X splash on a zombie, a
creeper or a skeleton.

## How it plays

- **Mutant Zombie**: 150 health, 12 attack, no knockback, steps up a block,
  1.8 x 3.2 blocks. Goes for iron golems, players and villagers; never hurts
  zombies.
  - Melee: four times in five a **slam** (25 ticks) sends a line of seismic
    waves 2 to 8 blocks ahead (9-12 damage on the first, 6-8 after, a push,
    sometimes Hunger II; grass and paths turn to dirt and plants break when
    `mob_griefing` is on); otherwise a **throw**: the target is flung up, the
    zombie leaps after it and slams it down for 12.
  - **Roar** now and then at a far target: pushes and hurts everything within
    14 blocks for 2-3, then zombies climb out of the ground around it for the
    next minute (husks by day, drowned in water).
  - **Three lives**: at 0 health it falls down for 7 seconds, nothing can hurt
    it, and it gets up with 37.5 health. The fourth fall is death. Fire while
    it is down (lava, fire, a flame arrow, a fire-aspect sword hit... any fire
    damage) burns its remaining lives away; so does dying while on fire.
  - Heals 2 health every 5 seconds at night. Doesn't burn in daylight, doesn't
    drown or turn into a drowned.
  - Drops a Hulk Hammer.
- **Mutant Creeper**: 120 health, 5 attack, no knockback, 1.99 x 2.8 blocks.
  Goes for players, cats and ocelots.
  - Melee hits push the target away.
  - **Jump attack** (now and then, target within 32 blocks): it leaps at the
    target and blows up where it lands (power 4, 6 when charged) without
    hurting itself.
  - **Charge** (once it has lost a sixth of its health, target 5 to 32 blocks
    off): it stands still for up to 100 ticks and heals; hitting it enough
    breaks the charge, and whoever hits it meanwhile gets thorns.
  - Explosions that aren't its own **heal** it by half their damage; lightning
    doesn't hurt it; a charged one doesn't burn.
  - **Death**: it swells for 5 seconds, pulling everything near towards it,
    then blows up (power 8, 12 when charged). Nothing hurts it meanwhile.
  - Drops experience only (upstream's loot table is empty; its creeper shard
    and minion egg are not ported).
- **Mutant Skeleton**: 150 health, 3 attack, 1.2 x 3.6 blocks. Goes for
  players, iron golems and wolves; nothing a Mutant Skeleton does hurts one.
  - Melee, three times in four a **swing** (14 ticks): everything within 3
    blocks in front of it takes 3-4 and is knocked far back; otherwise its
    **ribs** close on the target (20 ticks) for 9 and fling it away.
  - **Shot** (often, target more than 2 blocks off and in sight): after 26
    ticks one fast arrow that doesn't drop (speed 2.4, critical), one time in
    four poisoned, sometimes slowing instead. It aims worse just after a hit.
  - **Multi-shot** (now and then): it leaps, towards a far target and away
    from a near one, and looses three volleys of six arrows from the air.
  - Heals 2 health every 5 seconds at night. Doesn't burn in daylight.
  - **Death**: it shatters at once. Everything living within 3 blocks takes
    7, and its eighteen bones fly off as items: 8 limbs, 6 ribs, 2 shoulder
    pads, a pelvis and a skull.
- **Mutant Skeleton armour**: the skull is the helmet; the shoulders and limbs
  make the arms (`S S` / two rows of `L L`), six ribs a rib cage (`R R` three
  times), arms over rib cage the chestplate, pelvis and two limbs the
  leggings, two limbs the boots. Iron's defence and durability (2/5/6/2),
  enchantable; the boots add 0.2 jump strength and 2 blocks of safe fall, the
  leggings +40% speed and 0.5 step height. There is no custom worn texture, so
  a player wears it as iron. One kill drops what arms, rib cage, chestplate,
  boots and helmet take (8 limbs, 6 ribs, 2 pads, pelvis, skull); the
  leggings need two more limbs from a second kill.
- **Hulk Hammer**: 9 attack damage, very slow (-2.9 speed), 150 uses,
  enchantable. Right-click the top of a block: a line of waves 1.5 to 8 blocks
  the way you face (6-8 damage to everything on them), 25-tick cooldown, one
  use of durability.
- **Chemical X**: a thrown splash bottle (stacks to 16). Every grown zombie,
  creeper or skeleton within 4 blocks of the splash gets a spirit; 4 to 6 seconds later
  it turns into its mutant three times in four (the spirit fizzles out the fourth
  time). Recipe: any splash potion + a Hulk Hammer, shapeless.

## Status

### Done

- Content: entity types, items, the Mutant Skeleton armour set, creative tab,
  loot tables, damage types, tags (`#minecraft:zombies`,
  `#mutant_monsters:mutants`), recipes, lang, textures.
- Mutant Zombie, translated from upstream: model (`createBodyLayer`), every
  pose of `setupAnim` (walk, breathing, head, slam, roar, throw with its hit
  and landing, fall and revival, death flip), goals, the three attacks,
  seismic waves with upstream's line and ground search, zombie resurrection,
  lives and revival (saved in an entity attachment, so they survive a
  restart), throw-target immunity, night healing, hammer waves and wear,
  Chemical X conversion.
- Mutant Creeper, translated from upstream (`creeper.rs`): model, poses and
  the death shrink, goals, melee push, jump attack with its landing blast,
  charge with healing, hit counting and thorns, explosion healing, lightning
  and fire rules, the 100-tick death with its pull and final blast, the
  charged state (saved in an entity attachment), Chemical X conversion of
  creepers.
- Mutant Skeleton, translated from upstream (`skeleton.rs`): model (its
  three-piece spine with eighteen rib segments), every pose of `setupAnim`
  (walk, breathing, head, swing, rib crush with the ribs' swelling, shot,
  multi-shot), goals, the four attacks, its arrows (vanilla arrows with no
  gravity, owned by the skeleton, removed after 200 ticks), night healing,
  the shattering death with its 7 damage and the eighteen bones, Chemical X
  conversion of skeletons.
- Pure logic (lives, waves, ground search, attack choice, vectors, poses,
  messages, the creeper's choices and blasts, the skeleton's choices, arrows,
  pushes and poses) has 37 unit tests; they pass on the build VPS.
- Builds against Horizon `a85af177` on the build VPS: the dedicated-server
  engine (no default features) is built with the mod, and the client flavour
  (default features) type-checks (`cargo check`).
- **Headless smoke test on a scratch dedicated server** (flat world, one
  survival bot, commands on the console; `/opt/build/mm-smoke.sh` on the VPS,
  2026-11-12). Seen working in the server log:
  - Mutant Zombie: `/summon`, attributes (150 health), targeting the player,
    slams whose waves hurt the player (6-12), a throw, a roar, three falls
    and revivals at 37.5 health, invulnerable while down, death on the fourth
    fall with a Hulk Hammer dropped, instant death when burnt while down,
    lives kept across a server restart.
  - Mutant Creeper: `/summon` (120 health), melee hits on the player and on a
    Mutant Zombie, jump attacks with their landing blast (power 4), the
    100-tick death ending in the power 8 blast.
  - Chemical X: a splash on three zombies made one Mutant Zombie, a splash on
    two creepers one Mutant Creeper (the others' spirits were still pending
    or fizzled when the test moved on).
  - Mutant Skeleton (`/opt/build/mm-smoke-skel.sh`): `/summon` (150 health,
    3 attack), swings and rib crushes that hurt the player, single shots
    (plain, poisoned and slowing), multi-shot leaps with three volleys of six
    arrows (18 arrows counted, `NoGravity`, owned by the skeleton), night
    healing (140 to 144 in 12 s), the shattering death (7 damage to the
    player beside it, 18 bones thrown, all five kinds found), a Chemical X
    splash turning two skeletons into two Mutant Skeletons.

- **Independent re-run, 2026-11-13** (fresh server engine built from this
  folder, both smoke scripts): tests 37/37, all of the above seen again except
  one thing (the skeleton's night healing showed 134 to 136 in 12 s). Seen for
  the first time: the creeper's charge (it started one and ended it healed)
  and a TNT blast healing it (63 to 88.6). Not shown this time: lives kept
  across a restart (the
  script's `/kill` only downs Chemical X zombies, which have lives, so the
  leftover ones took the hit meant for the test zombie; the Mutant Zombie
  itself was saved and loaded back).

- **Skeleton armour set, 2026-11-14** (`tools/vps/mm-armour.sh`, the same
  scratch dedicated server): the content loads with no error, the six new
  items register and show their names (`Gave 1 [Mutant Skeleton Chestplate]`),
  the five recipes unlock (`Unlocked 1 recipe(s)`), and a full set worn on a
  player reads armor 15, armor toughness 0, speed 0.14 (0.1 x 1.4), step
  height 1.1, jump strength 0.62 and safe fall distance 5.0 — upstream's
  numbers. Crafting the pieces by hand and the worn look were not seen.

### Not done (known gaps)

- **Never seen on a client.** The models, textures and animations compile
  for the client flavour but no client was run, so nothing is visually
  checked (model shape, texture mapping, animation sync, the push a client
  applies to its own player). No real player has fought any of the bosses; try
  them on a test world first.
- Not exercised by the smoke test, so compiled but unverified: whether the
  skeleton's arrows hit and apply their potion (they were seen flying, the
  hits weren't told apart from its melee), the creeper's
  charged state and lightning, the Hulk Hammer's right-click waves, crafting
  Chemical X, throwing it by hand, natural spawning (`--spawns`), mob
  griefing by waves and blasts.
- Mutant Enderman, Spider Pig and Mutant Snow Golem: not ported (their Java
  is in `upstream/`). Chemical X does nothing to endermen or pigs.
- Mutant Skeleton: Horizon lets a mod mob copy a zombie, not a skeleton, so
  it is a zombie-class mob playing vanilla skeleton and crossbow sounds.  Not ported: the crossbow drawn in its hand, the flying bone entities (the
  bones are thrown as plain items, and don't hurt what they land on), the
  placeable skull block (the skull is only worn), `AvoidDamageGoal` (it
  doesn't run from fire or cactus), disabling a shield with the rib crush, the
  hunger arrows
  (an arrow here carries one potion at most: poison, else slowness, as a
  tipped arrow's, so with a tipped arrow's durations). A left-handed
  skeleton is drawn right-handed.
- Mutant Creeper: Horizon lets a mod mob copy a zombie, not a creeper, so it
  is a zombie-class mob playing vanilla creeper sounds. A trait mod can't
  set off an explosion itself, so each blast is a vanilla creeper summoned
  with a zero fuse and the blast's radius. Creeper minions, their egg and
  tracker and the creeper shard are not ported, so it never spawns minions
  and drops only experience.
- Upstream's own sounds and textures (All Rights Reserved): vanilla sounds and
  placeholder art instead. No spawn eggs, no advancements.
- Brewing: Horizon reads brewing recipes from the `minecraft` namespace only,
  so Chemical X is crafted (splash potion + hammer) instead of brewed from a
  thick splash potion.
- Lighting a downed zombie with flint and steel (upstream's interaction) isn't
  hooked; any fire damage or burning works instead.
- Mutant Zombie, not ported: the final 5-second vanish (it dies as a zombie
  dies, with the red overlay), breaking leaves when blocked, converting killed
  villagers into zombie villagers, the resurrected zombies following their
  summoner, TNT, bells, doors and redstone ore reacting to waves, the Chemical
  X splash explosion's damage, the hammer's item cooldown overlay (the
  cooldown is enforced but not drawn) and its Unbreaking handling, 50
  experience (it gives a zombie's).
- Approximations: Horizon's server hooks can't read a block's collision shape
  or the sky, so the wave ground search knows passable blocks by name, and a
  roar spot is refused two times in three by day wherever it is. Upstream sets
  the velocity of what it throws; here it is added (players get it through
  their own client, which needs this mod).
- The hitboxes are set by changing the mob's type copy in `register_goals`;
  whether every part of Horizon reads them from there is unverified.

## Build

Light content work (local):

```
python tools/make_textures.py   # content/assets/.../textures/*.png
python tools/pack.py            # target/mutant_monsters.hmod and .hnmod
cargo horizon-mod check target/mutant_monsters.hmod
cargo horizon-mod check target/mutant_monsters.hnmod
```

Native crate: on the build VPS. `/opt/build/mm-stage.sh` stages the crate
into `/opt/build/wt/mutant_monsters` (a worktree of Horizon `a85af177`) the
way the launcher does; `/opt/build/mm-check.sh full` runs `cargo check` for
both flavours plus `cargo test`; `/opt/build/mm-build2.sh` builds the
dedicated-server engine and checks the client flavour;
`CREEPER=1 /opt/build/mm-smoke.sh` and `/opt/build/mm-smoke-skel.sh` run the
headless smoke tests, and `/opt/build/mm-armour.sh` checks the skeleton armour
set (copies of all these scripts are in `tools/vps/`). The pure modules
also test alone:

```
rustc --edition 2024 --test mixins/mutant_monsters/api/src/logic.rs -o logic_tests && ./logic_tests
rustc --edition 2024 --test mixins/mutant_monsters/api/src/net.rs -o net_tests && ./net_tests
```

Pins: Horizon `a85af177a82504f357ff10d5d40d0b7230e2ac32`, horizon-mixins
`5a64af938ec86b00d53d16a408aabc0038fb3e57`, native hooks API `415`, game `26.3`.

## License

AGPL-3.0-only (`LICENSE`), as the original code. Credits, the pinned upstream
commit and what was translated are in `NOTICE.md`; the upstream Java is kept
unchanged in `upstream/`.
