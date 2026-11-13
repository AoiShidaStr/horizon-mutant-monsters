//! Pure game logic of the Mutant Zombie, translated from Mutant Monsters
//! (`MutantZombie`, `SeismicWave`, `ZombieResurrection`; AGPL-3.0, see NOTICE.md).
//!
//! Nothing here touches the game: the server half (`server.rs`) reads the
//! world into these types and applies what they decide, so all of it is
//! unit-tested with plain `rustc --test`.

/// The ids of the Mutant Zombie and the other kinds this mod names.
pub const MUTANT_ZOMBIE: &str = "mutant_monsters:mutant_zombie";
pub const CHEMICAL_X: &str = "mutant_monsters:chemical_x";
pub const HULK_HAMMER: &str = "mutant_monsters:hulk_hammer";

/// `MutantZombie.createAttributes`.
pub const MAX_HEALTH: f32 = 150.0;
pub const ATTACK_DAMAGE: f32 = 12.0;
/// `EntityType.Builder.sized(1.8F, 3.2F).eyeHeight(2.8F)`.
pub const WIDTH: f64 = 1.8;
pub const HEIGHT: f64 = 3.2;
pub const EYE_HEIGHT: f64 = 2.8;

/// `MutantZombie.REVIVE_AFTER_DEATH_TIME`.
pub const REVIVE_AFTER_DEATH_TIME: u32 = 140;
/// `DATA_REMAINING_LIVES`' default.
pub const LIVES: u8 = 3;

/// `EntityTypeTags.ZOMBIES` in 26.3, plus this mod's own entry: what
/// `MutantZombie.canHarm` spares.
pub const ZOMBIES: &[&str] = &[
    "minecraft:zombie",
    "minecraft:husk",
    "minecraft:drowned",
    "minecraft:zombie_villager",
    "minecraft:zombified_piglin",
    "minecraft:zoglin",
    "minecraft:zombie_horse",
    MUTANT_ZOMBIE,
];

pub fn is_zombie(kind: &str) -> bool {
    ZOMBIES.contains(&kind_id(kind))
}

/// A hook's entity kind as its id. Horizon `a85af177` names a mod's mob
/// `minecraft:<mod id>:<name>` in some hooks (`living_death`,
/// `living_conversion_pre`: `native_hooks::mob_kind` adds `minecraft:` to any
/// id), and `<mod id>:<name>` in others; both are read here.
pub fn kind_id(kind: &str) -> &str {
    match kind.strip_prefix("minecraft:") {
        Some(rest) if rest.contains(':') => rest,
        _ => kind,
    }
}

/// `MutantZombie.SLAM_GROUND_ANIMATION`, `THROW_ANIMATION`, `ROAR_ANIMATION`;
/// `Death` is the 140-tick fall and revival, which upstream draws from its
/// `deathTime` and this port sends as an animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Anim {
    #[default]
    None,
    Slam,
    Throw,
    Roar,
    Death,
}

impl Anim {
    pub fn id(self) -> u8 {
        match self {
            Anim::None => 0,
            Anim::Slam => 1,
            Anim::Throw => 2,
            Anim::Roar => 3,
            Anim::Death => 4,
        }
    }

    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            0 => Anim::None,
            1 => Anim::Slam,
            2 => Anim::Throw,
            3 => Anim::Roar,
            4 => Anim::Death,
            _ => return None,
        })
    }

    /// `EntityAnimation.duration()`.
    pub fn duration(self) -> u32 {
        match self {
            Anim::None => 0,
            Anim::Slam => 25,
            Anim::Throw => 15,
            Anim::Roar => 120,
            Anim::Death => REVIVE_AFTER_DEATH_TIME,
        }
    }
}

/// A small xorshift generator: the server draws the same kinds of numbers
/// upstream draws from `this.random`, without the game's generator.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15 | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// `nextInt(bound)`: 0 to bound - 1.
    pub fn int(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            (self.next_u64() % u64::from(bound)) as u32
        }
    }

    /// `nextFloat()`: 0 to 1.
    pub fn float(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn double(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
}

// --- lives and revival -------------------------------------------------------

/// A Mutant Zombie's lives (`DATA_REMAINING_LIVES`) and how long it has been
/// down (`deathTime` while it still has lives). Saved in an attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lives {
    pub lives: u8,
    /// 0 while standing; 1 to 140 while down.
    pub down: u32,
}

impl Default for Lives {
    fn default() -> Self {
        Lives { lives: LIVES, down: 0 }
    }
}

/// What a lethal hit does (`MutantZombie.die`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lethal {
    /// It falls and will get up again: the death is refused.
    GoDown,
    /// It stays down already (a hit on a downed zombie).
    StayDown,
    /// No lives left, burning, or `/kill`: it dies for good.
    Die,
}

impl Lives {
    pub fn is_down(&self) -> bool {
        self.down > 0
    }

    /// A lethal hit. `burning` is `isOnFire()` (a burning zombie loses its
    /// lives), `forced` a `/kill` or the void (`kill` sets the lives to 0).
    pub fn lethal(&mut self, burning: bool, forced: bool) -> Lethal {
        if burning || forced {
            self.lives = 0;
        }
        if self.lives == 0 {
            self.down = 0;
            return Lethal::Die;
        }
        if self.is_down() {
            return Lethal::StayDown;
        }
        self.down = 1;
        Lethal::GoDown
    }

    /// One tick while down (`tickDeath` with lives left): true the tick it
    /// stands up again, with a life less (`setHealth(getMaxHealth() / 4)`).
    pub fn tick_down(&mut self) -> bool {
        if !self.is_down() {
            return false;
        }
        self.down += 1;
        if self.down >= REVIVE_AFTER_DEATH_TIME {
            self.down = 0;
            self.lives = self.lives.saturating_sub(1);
            return true;
        }
        false
    }

    /// Set alight while down (`igniteForTicks` on a dying zombie): no lives
    /// left, so it dies when the server next looks.
    pub fn ignite(&mut self) {
        if self.is_down() {
            self.lives = 0;
        }
    }
}

/// The health a revived zombie gets back.
pub fn revive_health() -> f32 {
    MAX_HEALTH / 4.0
}

// --- attack choice --------------------------------------------------------------

/// `MutantZombie.doHurtTarget`: what a melee hit starts. `riding` is whether
/// the target rides the zombie (never here), `grounded` `onGround()` or in a
/// fluid. A slam four times in five; a throw otherwise; nothing when a slam
/// is picked in the air.
pub fn melee_choice(rng: &mut Rng, grounded: bool) -> Option<Anim> {
    if rng.int(5) != 0 {
        grounded.then_some(Anim::Slam)
    } else {
        Some(Anim::Throw)
    }
}

/// `customServerAiStep`: a slam now and then at a target close by and on the
/// same level.
pub fn ai_step_slam(rng: &mut Rng, zombie: [f64; 3], target: [f64; 3]) -> bool {
    (zombie[1] - target[1]).abs() <= 1.0 && dist_sqr(zombie, target) <= 49.0 && rng.int(20) == 0
}

/// `RoarGoal.canUse`, without `isAnimationPlaying` and the target check the
/// caller makes.
pub fn roar_choice(
    rng: &mut Rng,
    age: u32,
    on_ground: bool,
    resurrecting: bool,
    zombie: [f64; 3],
    target: [f64; 3],
) -> bool {
    age % 3 == 0
        && on_ground
        && !resurrecting
        && dist_sqr(zombie, target) > 16.0
        && rng.float() * 100.0 < 0.35
}

pub fn dist_sqr(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (x, y, z) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    x * x + y * y + z * z
}

/// The unit vector from `from` to `to` on the ground plane; (0, 0) when they
/// stand on the same spot (upstream divides by zero there and gets NaN, which
/// the game ignores).
pub fn flat_dir(from: [f64; 3], to: [f64; 3]) -> [f64; 2] {
    let (x, z) = (to[0] - from[0], to[2] - from[2]);
    let d = (x * x + z * z).sqrt();
    if d < 1e-6 { [0.0, 0.0] } else { [x / d, z / d] }
}

// --- the ground under a wave or a resurrection --------------------------------------------

/// What a block cell is to `getSuitableGround`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    /// `isEmptyBlock`.
    Air,
    /// A block with no collision (grass, flowers, torches...).
    Passable,
    /// A fluid; `lava` for lava.
    Fluid { lava: bool },
    /// `BlockTags.FIRE`.
    Fire,
    /// Anything with a collision box.
    Solid,
}

impl Cell {
    fn empty(self) -> bool {
        self == Cell::Air
    }

    fn no_collision(self) -> bool {
        !matches!(self, Cell::Solid)
    }
}

/// `ZombieResurrection.getSuitableGround(level, x, y, z, range, checkDay)`,
/// up to its daylight test (which needs the world: the caller makes it).
/// `cell(y)` reads the column at the wave's x and z. `None` is upstream's -1.
pub fn suitable_ground(
    mut cell: impl FnMut(i32) -> Cell,
    y: i32,
    range: i32,
    check_day: bool,
) -> Option<i32> {
    let mut i = y;
    loop {
        if (y - i).abs() > range {
            return None;
        }
        let here = cell(i);
        if here == Cell::Fire {
            return None;
        }
        if let Cell::Fluid { lava } = here {
            if !check_day || lava {
                break;
            }
        }
        if here.empty() {
            i -= 1;
        } else {
            let up = cell(i + 1);
            if up.empty() && here.no_collision() {
                i -= 1;
                break;
            }
            if up.empty() || up.no_collision() {
                break;
            }
            i += 1;
        }
    }
    Some(i)
}

/// A seismic wave: one block of the line the slam sends out
/// (`SeismicWave`). Every wave hurts what stands on it; `terrain` ones also
/// turn grass to dirt and knock plants off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wave {
    pub pos: [i32; 3],
    pub initial: bool,
    pub terrain: bool,
}

/// `SeismicWave.createWaves`: Bresenham's line from (x1, z1) to (x2, z2),
/// each block settled on the ground within 3 blocks of `y`. `ground(x, y, z)`
/// is `suitable_ground` at that column (range 3, no daylight test), `coin`
/// upstream's `nextInt(2) == 0`.
pub fn create_waves(
    x1: i32,
    z1: i32,
    x2: i32,
    z2: i32,
    y: i32,
    mut ground: impl FnMut(i32, i32, i32) -> Option<i32>,
    mut coin: impl FnMut() -> bool,
) -> Vec<Wave> {
    let mut list = Vec::new();
    let (mut dx, mut dz) = (x2 - x1, z2 - z1);
    let x_step = if dx < 0 { -1 } else { 1 };
    let z_step = if dz < 0 { -1 } else { 1 };
    dx = dx.abs();
    dz = dz.abs();
    let (mut x, mut z) = (x1, z1);
    let (dx2, dz2) = (dx * 2, dz * 2);
    let first_y = ground(x1, y, z1).unwrap_or(y);
    list.push(Wave { pos: [x1, first_y, z1], initial: true, terrain: true });
    let mut add = |x: i32, z: i32, list: &mut Vec<Wave>| {
        // `addWave`: upstream keeps -1 as the y of the extra wave when no
        // ground was found; that wave is out of the world, so it is left out.
        let found = ground(x, y, z);
        if let Some(gy) = found {
            list.push(Wave { pos: [x, gy, z], initial: false, terrain: true });
        }
        if coin() {
            if let Some(gy) = found {
                list.push(Wave { pos: [x, gy + 1, z], initial: false, terrain: false });
            }
        }
    };
    if dx2 >= dz2 {
        let mut error = dx;
        for _ in 0..dx {
            x += x_step;
            error += dz2;
            if error > dx2 {
                z += z_step;
                error -= dx2;
            }
            add(x, z, &mut list);
        }
    } else {
        let mut error = dz;
        for _ in 0..dz {
            z += z_step;
            error += dx2;
            if error > dz2 {
                x += x_step;
                error -= dz2;
            }
            add(x, z, &mut list);
        }
    }
    list
}

/// The box a wave hurts: the block above it, grown at random for the first
/// one (`updateMeleeGrounds`). `grow` is `nextDouble() * 0.75`.
pub fn wave_box(wave: &Wave, grow: f64) -> ([f64; 3], [f64; 3]) {
    let [x, y, z] = wave.pos;
    let (x, y, z) = (f64::from(x), f64::from(y), f64::from(z));
    let mut min = [x, y + 1.0, z];
    let mut max = [x + 1.0, y + 2.0, z + 1.0];
    if wave.initial {
        let (h, v) = (0.25 + grow, 0.25 + grow * 0.5);
        min = [min[0] - h, min[1] - v, min[2] - h];
        max = [max[0] + h, max[1] + v, max[2] + h];
    }
    (min, max)
}

/// What a wave deals: 9 to 12 for the first, 6 to 8 after.
pub fn wave_damage(rng: &mut Rng, initial: bool) -> f32 {
    if initial {
        (9 + rng.int(4)) as f32
    } else {
        (6 + rng.int(3)) as f32
    }
}

/// The slam's line: from two blocks in front of the zombie to eight
/// (`SlamGroundGoal.tick` at animation tick 12), and the hammer's from 1.5
/// to 8 (`HulkHammerItem.useOn`).
pub fn wave_line(at: [f64; 3], dir: [f64; 2], from: f64, to: f64) -> (i32, i32, i32, i32) {
    (
        (at[0] + dir[0] * from).floor() as i32,
        (at[2] + dir[1] * from).floor() as i32,
        (at[0] + dir[0] * to).floor() as i32,
        (at[2] + dir[1] * to).floor() as i32,
    )
}

/// `Vec3.directionFromRotation(0, yaw)` on the ground plane.
pub fn yaw_dir(yaw_degrees: f32) -> [f64; 2] {
    let r = f64::from(yaw_degrees).to_radians();
    [-r.sin(), r.cos()]
}

/// The block names the slam turns to dirt (`turns_into_dirt`).
pub fn turns_into_dirt(block: &str) -> bool {
    matches!(
        block,
        "minecraft:dirt_path" | "minecraft:farmland" | "minecraft:grass_block"
    )
}

/// A block name (`minecraft:oak_stairs[facing=east]` or bare) without its
/// properties.
pub fn block_name(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// What a block state string is to `suitable_ground`. Horizon's server hooks
/// can't ask for a block's collision shape, so blocks without one are known
/// by name: plants, flowers, torches, rails, buttons and the like.
pub fn classify(state: &str) -> Cell {
    let name = block_name(state);
    let path = name.split(':').nth(1).unwrap_or(name);
    match path {
        "air" | "cave_air" | "void_air" => return Cell::Air,
        "water" | "bubble_column" => return Cell::Fluid { lava: false },
        "lava" => return Cell::Fluid { lava: true },
        "fire" | "soul_fire" => return Cell::Fire,
        _ => {}
    }
    if state.contains("waterlogged=true") && !passable_name(path) {
        return Cell::Solid;
    }
    if passable_name(path) {
        Cell::Passable
    } else {
        Cell::Solid
    }
}

fn passable_name(path: &str) -> bool {
    const EXACT: &[&str] = &[
        "short_grass", "tall_grass", "fern", "large_fern", "dead_bush", "dandelion", "poppy",
        "blue_orchid", "allium", "azure_bluet", "oxeye_daisy", "cornflower", "lily_of_the_valley",
        "wither_rose", "sunflower", "lilac", "rose_bush", "peony", "torchflower", "pitcher_plant",
        "sugar_cane", "vine", "glow_lichen", "snow", "redstone_wire", "tripwire", "lever",
        "seagrass", "tall_seagrass", "kelp", "kelp_plant", "nether_sprouts", "crimson_roots",
        "warped_roots", "hanging_roots", "pink_petals", "wildflowers", "leaf_litter", "bush",
        "short_dry_grass", "tall_dry_grass", "firefly_bush", "cobweb", "light", "structure_void",
        "wheat", "carrots", "potatoes", "beetroots", "sweet_berry_bush", "brown_mushroom",
        "red_mushroom", "crimson_fungus", "warped_fungus", "spore_blossom", "cave_vines",
        "cave_vines_plant", "twisting_vines", "weeping_vines", "nether_wart", "moss_carpet",
    ];
    if EXACT.contains(&path) {
        return true;
    }
    path.ends_with("_tulip")
        || path.ends_with("_sapling")
        || path.ends_with("torch")
        || path.ends_with("_button")
        || path.ends_with("rail")
        || path.ends_with("_pressure_plate")
        || path.ends_with("_sign")
        || path.ends_with("_banner")
        || path.ends_with("_coral_fan")
        || path.ends_with("_propagule")
        || path.ends_with("_stem")
            && (path.starts_with("melon") || path.starts_with("pumpkin") || path.starts_with("attached"))
}

// --- zombie resurrection --------------------------------------------------------------

/// `ZombieResurrection`: a spot where a zombie climbs out of the ground once
/// `tick` runs out (100 to 139 ticks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resurrection {
    pub pos: [i32; 3],
    pub tick: i32,
}

impl Resurrection {
    pub fn new(rng: &mut Rng, pos: [i32; 3]) -> Self {
        Resurrection { pos, tick: 100 + rng.int(40) as i32 }
    }
}

/// `RoarGoal.tick` from animation tick 20 to 79, every tenth: a spot 1 to 8
/// blocks off on each axis, either side.
pub fn roar_spot(rng: &mut Rng, at: [f64; 3]) -> (i32, i32, i32) {
    let mut x = at[0].floor() as i32;
    let y = at[1].floor() as i32;
    let mut z = at[2].floor() as i32;
    x += (1 + rng.int(8) as i32) * if rng.bool() { 1 } else { -1 };
    z += (1 + rng.int(8) as i32) * if rng.bool() { 1 } else { -1 };
    (x, y - 1, z)
}

/// `ZombieResurrection.getZombieByLocation`, without the biome: a drowned
/// in water, a husk under the sky by day, else a zombie (one in twenty a
/// zombie villager).
pub fn resurrected_kind(rng: &mut Rng, in_water: bool, sunlit: bool) -> &'static str {
    if in_water {
        "minecraft:drowned"
    } else if sunlit {
        "minecraft:husk"
    } else if rng.float() < 0.05 {
        "minecraft:zombie_villager"
    } else {
        "minecraft:zombie"
    }
}

// --- the throw -------------------------------------------------------------------

/// `ThrowAttackGoal.start`: the target is thrown up and towards (away from) the zombie.
pub fn throw_launch(zombie: [f64; 3], target: [f64; 3]) -> [f64; 3] {
    let [x, z] = flat_dir(zombie, target);
    [x * 0.8, 1.6, z * 0.8]
}

/// `ThrowAttackGoal.tick` at the end of the animation: the zombie leaps at
/// the target.
pub fn throw_leap(zombie: [f64; 3], target: [f64; 3]) -> [f64; 3] {
    let (x, y, z) = (target[0] - zombie[0], target[1] - zombie[1], target[2] - zombie[2]);
    let d = (x * x + y * y + z * z).sqrt();
    if d < 1e-6 {
        return [0.0, 0.0, 0.0];
    }
    [x / d * 3.4, y / d * 1.4, z / d * 3.4]
}

/// Whether the leaping zombie reached the target (`d2 < d1`).
pub fn throw_reaches(zombie: [f64; 3], target_feet: [f64; 3]) -> bool {
    let reach = WIDTH * 2.0;
    dist_sqr(zombie, target_feet) < reach * reach
}

/// The slam down after the hit.
pub fn throw_slam_down(zombie: [f64; 3], target: [f64; 3]) -> [f64; 3] {
    let [x, z] = flat_dir(zombie, target);
    [x * 0.6, -1.2, z * 0.6]
}

/// The push of a seismic wave on what it hits.
pub fn wave_push(zombie: [f64; 3], hit: [f64; 3]) -> [f64; 3] {
    let [x, z] = flat_dir(zombie, hit);
    [x * 0.3, 0.04, z * 0.3]
}

/// The roar's push.
pub fn roar_push(zombie: [f64; 3], hit: [f64; 3]) -> [f64; 3] {
    let [x, z] = flat_dir(zombie, hit);
    [x * 0.7, 0.3, z * 0.7]
}

/// `level.isDarkOutside()` from the time of day: the overworld's night.
pub fn is_night(day_time: i64) -> bool {
    let t = day_time.rem_euclid(24000);
    (13000..23000).contains(&t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_lives_then_death() {
        let mut lives = Lives::default();
        for left in (1..=3).rev() {
            assert_eq!(lives.lives, left);
            assert_eq!(lives.lethal(false, false), Lethal::GoDown);
            assert_eq!(lives.lethal(false, false), Lethal::StayDown);
            let mut revived = false;
            for _ in 0..REVIVE_AFTER_DEATH_TIME {
                revived |= lives.tick_down();
            }
            assert!(revived);
            assert!(!lives.is_down());
        }
        assert_eq!(lives.lives, 0);
        assert_eq!(lives.lethal(false, false), Lethal::Die);
    }

    #[test]
    fn fire_or_kill_ends_it_at_once() {
        let mut lives = Lives::default();
        assert_eq!(lives.lethal(true, false), Lethal::Die);
        let mut lives = Lives::default();
        assert_eq!(lives.lethal(false, true), Lethal::Die);
        let mut lives = Lives::default();
        lives.lethal(false, false);
        lives.ignite();
        assert_eq!(lives.lives, 0);
        assert_eq!(lives.lethal(false, false), Lethal::Die);
    }

    #[test]
    fn revival_takes_140_ticks() {
        let mut lives = Lives::default();
        lives.lethal(false, false);
        let mut ticks = 1;
        while !lives.tick_down() {
            ticks += 1;
        }
        assert_eq!(ticks, REVIVE_AFTER_DEATH_TIME - 1);
        assert_eq!(lives.lives, 2);
    }

    #[test]
    fn waves_follow_the_line_and_the_ground() {
        // Flat ground at y = 63 everywhere.
        let waves = create_waves(0, 0, 6, 3, 64, |_, _, _| Some(63), || false);
        assert_eq!(waves[0], Wave { pos: [0, 63, 0], initial: true, terrain: true });
        assert_eq!(waves.len(), 7);
        assert_eq!(waves.last().unwrap().pos, [6, 63, 3]);
        for pair in waves.windows(2) {
            let d = (pair[1].pos[0] - pair[0].pos[0]).abs() + (pair[1].pos[2] - pair[0].pos[2]).abs();
            assert!((1..=2).contains(&d), "a gap in the line: {pair:?}");
        }
    }

    #[test]
    fn coin_adds_an_upper_wave_and_holes_are_skipped() {
        let waves = create_waves(0, 0, 0, -4, 64, |_, _, z| (z != -2).then_some(63), || true);
        assert!(waves.iter().any(|w| !w.terrain && w.pos == [0, 64, -1]));
        assert!(!waves.iter().any(|w| w.pos[2] == -2));
    }

    #[test]
    fn ground_is_found_under_and_over() {
        // Solid up to y = 60, grass on 61, air above.
        let column = |y: i32| match y {
            ..=60 => Cell::Solid,
            61 => Cell::Passable,
            _ => Cell::Air,
        };
        assert_eq!(suitable_ground(column, 63, 3, false), Some(60));
        assert_eq!(suitable_ground(column, 59, 3, false), Some(60));
        assert_eq!(suitable_ground(column, 70, 3, false), None);
        let burning = |y: i32| if y == 62 { Cell::Fire } else { column(y) };
        assert_eq!(suitable_ground(burning, 63, 3, false), None);
    }

    #[test]
    fn block_names_classify() {
        assert_eq!(classify("minecraft:air"), Cell::Air);
        assert_eq!(classify("minecraft:short_grass"), Cell::Passable);
        assert_eq!(classify("minecraft:red_tulip"), Cell::Passable);
        assert_eq!(classify("minecraft:grass_block[snowy=false]"), Cell::Solid);
        assert_eq!(classify("minecraft:lava[level=0]"), Cell::Fluid { lava: true });
        assert!(turns_into_dirt(block_name("minecraft:grass_block[snowy=false]")));
    }

    #[test]
    fn attack_choice_is_mostly_slams() {
        let mut rng = Rng::new(7);
        let mut slams = 0;
        let mut throws = 0;
        for _ in 0..1000 {
            match melee_choice(&mut rng, true) {
                Some(Anim::Slam) => slams += 1,
                Some(Anim::Throw) => throws += 1,
                _ => unreachable!(),
            }
        }
        assert!(slams > 700 && throws > 120, "{slams} slams, {throws} throws");
        assert_eq!(melee_choice(&mut Rng::new(1), false).filter(|a| *a == Anim::Slam), None);
    }

    #[test]
    fn throw_vectors() {
        let launch = throw_launch([0.0, 64.0, 0.0], [3.0, 64.0, 4.0]);
        assert!((launch[0] - 0.48).abs() < 1e-9 && (launch[2] - 0.64).abs() < 1e-9);
        assert_eq!(launch[1], 1.6);
        assert!(throw_reaches([0.0, 64.0, 0.0], [3.0, 64.0, 0.0]));
        assert!(!throw_reaches([0.0, 64.0, 0.0], [4.0, 64.0, 0.0]));
        let leap = throw_leap([0.0, 0.0, 0.0], [0.0, 0.0, 10.0]);
        assert!((leap[2] - 3.4).abs() < 1e-9);
    }

    #[test]
    fn anim_ids_round_trip() {
        for anim in [Anim::None, Anim::Slam, Anim::Throw, Anim::Roar, Anim::Death] {
            assert_eq!(Anim::from_id(anim.id()), Some(anim));
        }
        assert_eq!(Anim::from_id(9), None);
    }

    #[test]
    fn kinds_with_and_without_the_extra_namespace() {
        assert_eq!(kind_id("minecraft:mutant_monsters:mutant_zombie"), MUTANT_ZOMBIE);
        assert_eq!(kind_id(MUTANT_ZOMBIE), MUTANT_ZOMBIE);
        assert_eq!(kind_id("minecraft:zombie"), "minecraft:zombie");
        assert!(is_zombie("minecraft:mutant_monsters:mutant_zombie"));
    }

    #[test]
    fn night_and_yaw() {
        assert!(is_night(18000));
        assert!(!is_night(6000));
        let south = yaw_dir(0.0);
        assert!(south[0].abs() < 1e-9 && (south[1] - 1.0).abs() < 1e-9);
    }
}
