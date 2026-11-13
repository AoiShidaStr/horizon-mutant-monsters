# Notices

## The original mod

Mutant Monsters is by shcott21, Chumbanotz, Fuzs and tdstress
(https://github.com/Fuzss/mutantmonsters, branch `26.3.x`, commit
`0a8e21bd9c3d0c91567c955bb97fad23b423021d`, 2026-10-17). Its code is released
under the GNU Affero General Public License v3.0 (Modrinth lists
AGPL-3.0-only; the repository's `gradle.properties` says AGPL-3.0-or-later;
this port treats it as AGPL-3.0-only). Its textures, sounds and `pack.png`
are "Copyright (c) 2024-2026 @heyitsfuzs. All Rights Reserved."
(`upstream/LICENSE-ASSETS.md`).

## What this port contains, and where it comes from

This folder is a modified version of Mutant Monsters, translated from Java to
Rust for Horizon (a Rust rewrite of Minecraft Java 26.3), started 2026-11-11.

- `upstream/java/`, `upstream/generated-data/`: the original Java code and its
  datagen output at the pinned commit, unchanged, kept here as the source the
  translation was made from. `upstream/LICENSE.md` is the original licence
  file, `upstream/LICENSE-ASSETS.md` the assets notice. No upstream texture,
  sound or `pack.png` is in this folder.
- `mixins/mutant_monsters/api/src/`: Rust translated from that Java:
  - `model.rs`, `pose.rs`: `MutantZombieModel` (`createBodyLayer`,
    `setupAnim`, `animateMelee`, `animateRoar`, `animateThrow`,
    `animateDeath`) and `MutantZombieRenderer` (scale, offset, death flip).
  - `goals.rs`: `MutantZombie.registerGoals`, `MutantMeleeAttackGoal`,
    `doHurtTarget`, `customServerAiStep`, `RoarGoal.canUse`.
  - `server.rs`, `logic.rs`: `SlamGroundGoal`, `ThrowAttackGoal`, `RoarGoal`,
    `SeismicWave`, `ZombieResurrection`, `MutantZombie` (`die`, `tickDeath`,
    `hurtServer`, `updateMeleeGrounds`, night healing), `HulkHammerItem`,
    `PlayerEventsHandler` (player waves), `ChemicalXMobEffect` and
    `SkullSpirit` (zombie conversion).
  - `creeper.rs`: `MutantCreeper` (`registerGoals`, `doHurtTarget`,
    `hurtServer`, `die`, `tickDeath`, `thunderHit`), `JumpAttackGoal`,
    `ChargeAttackGoal`, `MutatedExplosionHelper` (as zero-fuse vanilla
    creepers), `MutantCreeperModel` and `MutantCreeperRenderer`, and the
    creeper case of `ChemicalXMobEffect`.
  - `skeleton.rs`: `MutantSkeleton` (`registerGoals`, `doHurtTarget`,
    `hurtServer`, `die`, `aiStep`), its `MeleeGoal`, `ConstrictRibsGoal`,
    `ShootGoal` and `MultiShotGoal`, `MutantArrow` (as vanilla arrows),
    `MutantSkeletonBodyPart.BodyPart` (as item drops),
    `EntityUtil.getHeadAngle`, `Animator`'s phases, `MutantSkeletonModel`
    (with `Spine`) and `MutantSkeletonRenderer`, and the skeleton case of
    `ChemicalXMobEffect`.
  - `net.rs`: stands in for `ClientboundAnimationMessage` and
    `EntityUtil.sendPlayerVelocityPacket`.
- `content/data/`: re-derived from the AGPL datagen output (entity loot table,
  damage types, tags, Hulk Hammer components from `ModItems`/`HulkHammerItem`).
  The Chemical X crafting recipe is new (Horizon reads brewing recipes only
  from the `minecraft` namespace, so upstream's brewing mix can't be added).
- `content/assets/.../textures/`: original placeholder art drawn by
  `tools/make_textures.py` (seeded noise and flat shapes, no source image).
- `content/assets/.../lang/`: written for this port (en_us, fr_fr).
- Sounds: upstream's sound files are All Rights Reserved, so vanilla sound
  events are played instead (ravager attack/roar, zombie, creeper, skeleton, crossbow, explosion).

No NeoForge, Fabric or Puzzles Lib code is used or required.

## Licence of this port

This port is distributed under the GNU Affero General Public License v3.0 only
(`LICENSE`), as the code it was translated from. Horizon itself is MIT; a game
built with this native mod carries AGPL-3.0 code, so whoever runs it for other
players must offer them the corresponding source (AGPL section 13): the
`.hnmod` holds the full Rust source with this notice and the licence, and the
README gives where to get it. The textures, lang files and the new recipe in
this folder are released under the same licence.
