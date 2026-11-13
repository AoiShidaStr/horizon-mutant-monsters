//! The Mutant Creeper: `MutantCreeper`, `MutantCreeperModel` and
//! `MutantCreeperRenderer`, translated from Mutant Monsters (AGPL-3.0, see
//! NOTICE.md) onto Horizon. One file holds its pure logic (tested), its goals,
//! its server half and its client half, as `logic.rs`, `goals.rs`,
//! `server.rs`, `model.rs`/`pose.rs` and `client.rs` do for the Mutant Zombie.
//!
//! Differences forced by Horizon (see README.md, "Not done"):
//! - Horizon lets a mod type copy a zombie, not a creeper: the Mutant Creeper
//!   is a silent zombie-class mob that plays vanilla creeper sounds (upstream's
//!   own sounds are All Rights Reserved).
//! - A trait mod can't set off an explosion itself (`native_explode` isn't in
//!   the mod API), so each blast is a vanilla creeper summoned with a zero
//!   fuse and the blast's radius (`ExplosionRadius`): it explodes on its
//!   first tick, with the creeper's own rules (`mob_griefing`).
//! - Creeper minions, their egg and tracker, and the creeper shard are not
//!   ported, so `SpawnMinionsGoal` is left out and the creeper drops nothing
//!   but experience (its loot table is upstream's, empty).
use crate::brain::TargetSnap;
use crate::logic::{self, Rng};
use crate::net::{AnimMessage, PushMessage};
use horizon_client::mobs::{
    Mob, Species, Who,
    goals::{Cx, Float, Goal, HurtByTarget, LOOK, LookAtPlayer, MOVE, NearestTarget, RandomLookAround, Stroll},
};
use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, BlockEntityDraw, Compound, CubeListBuilder, Flow, Holder, Host,
    LayerDefinition, MeshDefinition, ModelPart, NativeMobRender, NativeServerPlayer, ParticleOptions,
    ParticleType, PartPose, Tag,
};
use std::{
    collections::{HashMap, HashSet},
    f32::consts::PI,
    sync::{Arc, Mutex, OnceLock},
};

pub const MUTANT_CREEPER: &str = "mutant_monsters:mutant_creeper";
/// `MutantCreeper.createAttributes`.
pub const MAX_HEALTH: f32 = 120.0;
pub const ATTACK_DAMAGE: f32 = 5.0;
/// `EntityType.Builder.sized(1.99F, 2.8F).eyeHeight(2.6F)`.
pub const WIDTH: f64 = 1.99;
pub const HEIGHT: f64 = 2.8;
pub const EYE_HEIGHT: f64 = 2.6;
pub const MAX_CHARGE_TIME: u32 = 100;
pub const MAX_DEATH_TIME: u32 = 100;
/// The swing of a melee hit (`getCurrentSwingDuration`).
pub const SWING_TIME: u32 = 6;

/// Server to client: what a Mutant Creeper is doing (an `AnimMessage`, with
/// `act` ids and the `CHARGED` flag), in place of `DATA_STATUS` and
/// `deathTime`.
pub const CHANNEL: &str = "mutant_monsters:creeper";
pub const CHARGED: u8 = 1;

pub const LAYER: &str = "mutant_monsters:mutant_creeper#main";
pub const TEXTURE: &str = "mutant_monsters:entity/mutant_creeper";

const HOSTILE: usize = 5;
const VIEW: f64 = 96.0;

/// What a Mutant Creeper is doing, as its client draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Act {
    #[default]
    None,
    /// A melee hit (`swing`).
    Swing,
    /// `isJumpAttacking`.
    Jump,
    /// `isCharging`.
    Charge,
    /// The 100-tick death (`tickDeath`).
    Death,
}

impl Act {
    pub fn id(self) -> u8 {
        match self {
            Act::None => 0,
            Act::Swing => 1,
            Act::Jump => 2,
            Act::Charge => 3,
            Act::Death => 4,
        }
    }

    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            0 => Act::None,
            1 => Act::Swing,
            2 => Act::Jump,
            3 => Act::Charge,
            4 => Act::Death,
            _ => return None,
        })
    }
}

// --- pure logic -------------------------------------------------------------------

/// `JumpAttackGoal.canUse`: on the ground, the target within 32 blocks,
/// 0.9% a tick.
pub fn jump_choice(rng: &mut Rng, on_ground: bool, dist_sqr: f64, busy: bool) -> bool {
    on_ground && !busy && dist_sqr <= 1024.0 && rng.float() * 100.0 < 0.9
}

/// `ChargeAttackGoal.canUse`: hurt by a sixth of its health or more, the
/// target 5 to 32 blocks off, 0.7% a tick.
pub fn charge_choice(rng: &mut Rng, health: f32, dist_sqr: f64, busy: bool) -> bool {
    !busy
        && MAX_HEALTH - health >= MAX_HEALTH / 6.0
        && (25.0..=1024.0).contains(&dist_sqr)
        && rng.float() * 100.0 < 0.7
}

/// `JumpAttackGoal.start`: the leap at the target.
pub fn jump_velocity(creeper: [f64; 3], target: [f64; 3]) -> [f64; 3] {
    [(target[0] - creeper[0]) * 0.2, 1.4, (target[2] - creeper[2]) * 0.2]
}

/// `doHurtTarget`'s push on what it hits.
pub fn melee_push(creeper: [f64; 3], target: [f64; 3]) -> [f64; 3] {
    let (x, y, z) = (target[0] - creeper[0], target[1] - creeper[1], target[2] - creeper[2]);
    let d = (x * x + y * y + z * z).sqrt();
    if d <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    [x / d * 0.5, y / d * 0.05 + 0.15, z / d * 0.5]
}

/// `tickDeath`: everything near is drawn in, harder as the end comes.
pub fn death_pull(creeper: [f64; 3], other: [f64; 3], death_time: u32) -> [f64; 3] {
    let (x, y, z) = (creeper[0] - other[0], creeper[1] - other[1], creeper[2] - other[2]);
    let d = (x * x + y * y + z * z).sqrt();
    if d <= 1e-6 {
        return [0.0, 0.0, 0.0];
    }
    let s = f64::from(death_time) / f64::from(MAX_DEATH_TIME) * 0.09;
    [x / d * s, y / d * s, z / d * s]
}

/// The death blast's power (`tickDeath`), and how far it pulls (1.5 times).
pub fn death_power(charged: bool) -> f32 {
    if charged { 12.0 } else { 8.0 }
}

/// The jump attack's blast (`MutatedExplosionHelper.explode(this, 4 or 6)`).
pub fn jump_power(charged: bool) -> f32 {
    if charged { 6.0 } else { 4.0 }
}

/// `MutantCreeperRenderer.scale`: 1.2, shrinking to 0.8 as it dies.
pub fn render_scale(death_time: f32) -> f32 {
    1.2 - (death_time / MAX_DEATH_TIME as f32).clamp(0.0, 1.0) * 0.4
}

/// `ChargeAttackGoal`'s state between ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Charge {
    pub time: u32,
    /// Hits it can take before the charge breaks (`chargeHits`).
    pub hits: i32,
    /// Ends with a lightning bolt when the target comes close
    /// (`summonLightning`).
    pub lightning: bool,
}

/// How a charge ends (`ChargeAttackGoal.stop`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChargeEnd {
    /// The target came within 5 blocks: lightning strikes and charges it.
    Lightning,
    /// It charged for the full 100 ticks: 30 health back.
    Heal,
    /// Hit too often: nothing.
    Broken,
}

impl Charge {
    pub fn new(hits: i32) -> Self {
        Charge { time: 0, hits, lightning: false }
    }

    /// `canContinueToUse`.
    pub fn continues(&self, target_close: bool) -> bool {
        !(self.lightning && target_close) && self.time < MAX_CHARGE_TIME && self.hits > 0
    }

    pub fn end(&self, target_close: bool) -> ChargeEnd {
        if self.lightning && target_close {
            ChargeEnd::Lightning
        } else if self.time >= MAX_CHARGE_TIME {
            ChargeEnd::Heal
        } else {
            ChargeEnd::Broken
        }
    }
}

/// `damageSource.is(DamageTypeTags.IS_EXPLOSION)`.
pub fn is_explosion(damage_type: &str) -> bool {
    matches!(damage_type, "minecraft:explosion" | "minecraft:player_explosion" | "minecraft:fireworks")
}

fn is_fire(damage_type: &str) -> bool {
    matches!(
        damage_type,
        "minecraft:in_fire" | "minecraft:on_fire" | "minecraft:lava" | "minecraft:campfire" | "minecraft:hot_floor"
    )
}

/// `DamageTypeTags.WITCH_RESISTANT_TO`: no thorns for these.
fn witch_resistant(damage_type: &str) -> bool {
    matches!(
        damage_type,
        "minecraft:magic" | "minecraft:indirect_magic" | "minecraft:sonic_boom" | "minecraft:thorns"
    )
}

// --- what the goals and the server share --------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Brain {
    pos: [f64; 3],
    on_ground: bool,
    in_fluid: bool,
    health: f32,
    age: u32,
    target: Option<TargetSnap>,
    // Asked by the goals, done by the server.
    jump: bool,
    charge: bool,
    melee: bool,
    // Set by the server.
    jumping: bool,
    charging: bool,
    dying: bool,
}

impl Brain {
    fn busy(&self) -> bool {
        self.jumping || self.charging || self.dying
    }
}

fn brains() -> &'static Mutex<HashMap<u32, Brain>> {
    static B: OnceLock<Mutex<HashMap<u32, Brain>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(HashMap::new()))
}

fn with<R>(id: u32, f: impl FnOnce(&mut Brain) -> R) -> Option<R> {
    let mut map = brains().lock().ok()?;
    Some(f(map.entry(id).or_default()))
}

fn get(id: u32) -> Option<Brain> {
    brains().lock().ok()?.get(&id).cloned()
}

/// What hits on a creeper left for the server (the damage hook has no host).
#[derive(Clone, Debug, Default)]
struct Hits {
    heal: f32,
    thorns: Vec<u32>,
    charge_hits: i32,
}

fn hits() -> &'static Mutex<HashMap<i32, Hits>> {
    static H: OnceLock<Mutex<HashMap<i32, Hits>>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Creepers that are dying (no hit lands), and those whose death is being
/// carried out (`living_death` lets it through).
fn dying() -> &'static Mutex<HashSet<i32>> {
    static D: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(HashSet::new()))
}

fn finishing() -> &'static Mutex<HashSet<u32>> {
    static F: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Charged creepers, read by the damage hook (`fireImmune`).
fn charged_set() -> &'static Mutex<HashSet<i32>> {
    static C: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashSet::new()))
}

/// The vanilla creepers summoned as blasts: their explosions don't heal a
/// Mutant Creeper (`!(source.getEntity() instanceof MutantCreeper)`).
fn blasters() -> &'static Mutex<HashSet<i32>> {
    static B: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(HashSet::new()))
}

// --- goals ------------------------------------------------------------------------------

/// `MutantCreeper.registerGoals`, with its size. Asked for every new mob.
pub fn register(mob: &mut Mob) {
    if mob.kind.id != MUTANT_CREEPER {
        return;
    }
    let kind = Arc::make_mut(&mut mob.kind);
    kind.width = WIDTH;
    kind.height = HEIGHT;
    kind.eye_height = EYE_HEIGHT;
    kind.flags.burns_in_day = false;

    let seed = u64::from(mob.id).wrapping_mul(0x2545_F491_4F6C_DD1D);
    let goals = &mut mob.ai.goals;
    goals.clear();
    goals.add(0, Float);
    goals.add(0, Sense { rng: Rng::new(seed ^ 3) });
    goals.add(0, Busy);
    goals.add(2, Melee { speed: 1.3, recalc: 0, cooldown: 0, rng: Rng::new(seed ^ 4) });
    goals.add(4, Stroll::avoiding_water(1.0));
    goals.add(5, LookAtPlayer::new(8.0));
    goals.add(6, RandomLookAround::default());

    let targets = &mut mob.ai.targets;
    targets.clear();
    targets.add(0, HurtByTarget::new());
    targets.add(1, NearestTarget::player(true).unseen_memory(200));
    targets.add(2, NearestTarget::species(&[Species::Cat, Species::Ocelot], true));
}

fn snapshot(m: &Mob, cx: &Cx) -> Option<TargetSnap> {
    let who = m.ai.target?;
    let t = cx.w.target_info(who)?;
    Some(TargetSnap {
        mob: match who {
            Who::Mob(id) => Some(id),
            Who::Player => None,
        },
        is_player: t.is_player,
        pos: [t.position.x, t.position.y, t.position.z],
        eye_y: t.eye_y,
        alive: t.alive,
        creative: t.creative,
    })
}

/// Writes the mob's state for the server half, and makes the jump and charge
/// decisions (`JumpAttackGoal.canUse`, `ChargeAttackGoal.canUse`).
#[derive(Clone, Debug)]
struct Sense {
    rng: Rng,
}

impl Goal for Sense {
    fn flags(&self) -> u8 {
        0
    }
    fn can_use(&mut self, _: &mut Mob, _: &mut Cx) -> bool {
        true
    }
    fn can_continue(&mut self, _: &mut Mob, _: &mut Cx) -> bool {
        true
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let target = snapshot(m, cx).filter(|t| t.alive);
        let pos = [m.position.x, m.position.y, m.position.z];
        let (on_ground, in_fluid, health, age) = (m.on_ground, m.fluids.in_water, m.health, m.age);
        let rng = &mut self.rng;
        with(m.id, |b| {
            b.pos = pos;
            b.on_ground = on_ground;
            b.in_fluid = in_fluid;
            b.health = health;
            b.age = age;
            b.target = target;
            let Some(t) = target else { return };
            if b.busy() || b.jump || b.charge {
                return;
            }
            let d = logic::dist_sqr(pos, t.pos);
            if jump_choice(rng, on_ground, d, false) {
                b.jump = true;
            } else if charge_choice(rng, health, d, false) {
                b.charge = true;
            }
        });
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

/// Holds the creeper still while it charges (`ChargeAttackGoal`'s MOVE and
/// LOOK) or dies.
#[derive(Clone, Debug, Default)]
struct Busy;

impl Goal for Busy {
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn interruptable(&self) -> bool {
        false
    }
    fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
        get(m.id).is_some_and(|b| b.charging || b.dying)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.nav_stop();
        if get(m.id).is_some_and(|b| b.charging) {
            if let Some(t) = snapshot(m, cx) {
                m.look_at_speed([t.pos[0], t.eye_y, t.pos[2]], 30.0, 30.0);
            }
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

/// `MutantMeleeAttackGoal(this, 1.3)`: walk at the target and hit it every
/// 20 ticks in reach; the server deals the hit (`doHurtTarget`).
#[derive(Clone, Debug)]
struct Melee {
    speed: f64,
    recalc: i32,
    cooldown: i32,
    rng: Rng,
}

impl Goal for Melee {
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if get(m.id).is_some_and(|b| b.charging || b.dying) {
            return false;
        }
        match m.target_info(cx.w) {
            Some(t) if t.alive => true,
            Some(_) => {
                m.ai.target = None;
                false
            }
            None => false,
        }
    }
    fn can_continue(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        self.can_use(m, cx)
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        m.ai.aggressive = true;
        self.recalc = 0;
    }
    fn stop(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
        if m.ai.target.is_none() {
            m.ai.aggressive = false;
        }
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        let Some(who) = m.ai.target else { return };
        let Some(t) = cx.w.target_info(who) else { return };
        m.look_at_speed([t.position.x, t.eye_y, t.position.z], 30.0, 30.0);
        self.recalc -= 1;
        if self.recalc <= 0 {
            self.recalc = 4 + self.rng.int(7) as i32;
            m.nav_move_to_entity(cx.w, [t.position.x, t.position.y, t.position.z], 0, self.speed);
        }
        self.cooldown = (self.cooldown - 1).max(0);
        if self.cooldown == 0 && m.within_melee_range(cx.w, &t) && m.sees(cx.w, who) {
            self.cooldown = 20;
            with(m.id, |b| b.melee = true);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

// --- server -------------------------------------------------------------------------------

/// What is saved with a creeper: whether it is charged (`Powered`).
static SAVED: AttachmentType<bool> = AttachmentType {
    namespace: "mutant_monsters",
    name: "mutant_creeper",
    default: || false,
    codec: Some(AttachmentCodec {
        encode: |c| {
            let mut t = Compound::new();
            t.insert("powered".into(), Tag::Byte(i8::from(*c)));
            Ok(t)
        },
        decode: |t| match t.get(&"powered".into()) {
            Some(Tag::Byte(b)) => Ok(*b != 0),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: false,
};

struct Runtime {
    charged: bool,
    jump_tick: u32,
    charge: Charge,
    death_time: u32,
    last_health: f32,
    killed_by_player: bool,
    rng: Rng,
    last_seen: u64,
}

fn runtimes() -> &'static Mutex<HashMap<(String, u32), Runtime>> {
    static RT: OnceLock<Mutex<HashMap<(String, u32), Runtime>>> = OnceLock::new();
    RT.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `MutantCreeper.hurtServer` and `fireImmune`, for the damage hook: a dying
/// creeper takes nothing, explosions heal it by half (but its own), a charged
/// one doesn't burn, lightning only charges it (`thunderHit`), and while it
/// charges each hit counts and the hitter gets thorns.
pub fn living_incoming_damage(entity: i32, damage_type: &str, causing: i32, direct: i32, amount: f32) -> Flow {
    if dying().lock().map(|s| s.contains(&entity)).unwrap_or(false) {
        return Flow::Cancel;
    }
    if is_explosion(damage_type) {
        let own = blasters().lock().map(|s| s.contains(&causing) || s.contains(&direct)).unwrap_or(false);
        if !own && amount > 0.0 {
            if let Ok(mut h) = hits().lock() {
                h.entry(entity).or_default().heal += amount / 2.0;
            }
        }
        return Flow::Cancel;
    }
    if damage_type == "minecraft:lightning_bolt" {
        return Flow::Cancel;
    }
    let charged = charged_set().lock().map(|s| s.contains(&entity)).unwrap_or(false);
    if charged && is_fire(damage_type) {
        return Flow::Cancel;
    }
    let charging = brains().lock().ok().and_then(|m| m.get(&(entity as u32)).map(|b| b.charging)).unwrap_or(false);
    if charging {
        if let Ok(mut h) = hits().lock() {
            let e = h.entry(entity).or_default();
            if direct > 0 && direct != entity && !witch_resistant(damage_type) {
                e.thorns.push(direct as u32);
            }
            if amount > 0.0 {
                e.charge_hits += 1;
            }
        }
    }
    Flow::Continue
}

/// `MutantCreeper.die`: it doesn't die at once but swells for 100 ticks
/// (`tickDeath`), then blows up.
pub fn living_death(host: &mut dyn Host, entity: u32, attacker: u32) -> Flow {
    if finishing().lock().map(|mut s| s.remove(&entity)).unwrap_or(false) {
        return Flow::Continue;
    }
    let already = dying().lock().map(|s| s.contains(&(entity as i32))).unwrap_or(false);
    if already {
        return Flow::Cancel;
    }
    let by_player = attacker > 0 && host.server_players().iter().any(|p| p.entity_id == attacker as i32);
    if let Ok(mut s) = dying().lock() {
        s.insert(entity as i32);
    }
    with(entity, |b| {
        b.dying = true;
        b.charging = false;
        b.jumping = false;
        b.jump = false;
        b.charge = false;
    });
    if let Ok(mut map) = runtimes().lock() {
        if let Some((_, rt)) = map.iter_mut().find(|((_, i), _)| *i == entity) {
            rt.death_time = 0;
            rt.killed_by_player |= by_player;
        }
    }
    let pos = host.entity(entity).and_then(|e| host.entity_position(e));
    if let Some(pos) = pos {
        let _ = host.play_sound("minecraft:entity.creeper.death", pos, HOSTILE, 2.0, 1.0);
        let charged = charged_set().lock().map(|s| s.contains(&(entity as i32))).unwrap_or(false);
        broadcast(host, None, entity, pos, Act::Death, 0, charged);
    }
    super_trace(format_args!("creeper {entity} starts dying (by player {by_player})"));
    Flow::Cancel
}

fn super_trace(args: std::fmt::Arguments<'_>) {
    crate::server::trace_line(args);
}

fn load_charged(host: &mut dyn Host, id: u32) -> bool {
    SAVED.get(host, Holder::Entity(id)).unwrap_or(false)
}

fn set_charged(host: &mut dyn Host, id: u32, rt: &mut Runtime, charged: bool) {
    rt.charged = charged;
    let _ = SAVED.set(host, Holder::Entity(id), charged);
    if let Ok(mut s) = charged_set().lock() {
        if charged {
            s.insert(id as i32);
        } else {
            s.remove(&(id as i32));
        }
    }
}

/// One tick of a Mutant Creeper on the server, after the game ticked it.
pub fn entity_tick_post(host: &mut dyn Host, dim: &str, id: u32, now: u64) {
    let Some(brain) = get(id) else { return };
    let key = (dim.to_owned(), id);
    let first = runtimes().lock().map(|m| !m.contains_key(&key)).unwrap_or(false);
    if first {
        let charged = load_charged(host, id);
        let mut rt = Runtime {
            charged: false,
            jump_tick: 0,
            charge: Charge::new(3 + (u64::from(id) % 3) as i32),
            death_time: 0,
            last_health: brain.health,
            killed_by_player: false,
            rng: Rng::new(u64::from(id) ^ now ^ 0xC4EE),
            last_seen: now,
        };
        set_charged(host, id, &mut rt, charged);
        if let Ok(mut map) = runtimes().lock() {
            map.insert(key.clone(), rt);
        }
        if let Some(e) = host.entity(id) {
            let mut nbt = Compound::new();
            nbt.insert("IsBaby".into(), Tag::Byte(0));
            nbt.insert("CanPickUpLoot".into(), Tag::Byte(0));
            nbt.insert("CanBreakDoors".into(), Tag::Byte(0));
            // The zombie's groans are not a creeper's: its sounds are played here.
            nbt.insert("Silent".into(), Tag::Byte(1));
            let _ = host.entity_merge_nbt(e, nbt);
        }
        broadcast(host, None, id, brain.pos, Act::None, 0, charged);
    }
    let players = host.server_players();
    let Ok(mut map) = runtimes().lock() else { return };
    let Some(rt) = map.get_mut(&key) else { return };
    rt.last_seen = now;
    let pos = brain.pos;

    // What the damage hook left.
    let hit = hits().lock().ok().and_then(|mut h| h.remove(&(id as i32))).unwrap_or_default();
    if hit.heal > 0.0 && brain.health > 0.0 && !brain.dying {
        let _ = host.set_mob_health(dim, id, (brain.health + hit.heal).min(MAX_HEALTH));
        hearts(host, pos, (hit.heal / 2.0) as i32);
    }
    for attacker in hit.thorns {
        let _ = host.hurt_entity(dim, attacker, 2.0, "minecraft:thorns", Some(id));
    }
    rt.charge.hits -= hit.charge_hits;
    if brain.health < rt.last_health - 0.01 && !brain.dying {
        let _ = host.play_sound("minecraft:entity.creeper.hurt", pos, HOSTILE, 2.0, 0.6 + rt.rng.float() * 0.2);
    }
    rt.last_health = brain.health;

    if brain.dying {
        let done = tick_death(host, dim, id, pos, rt, &players);
        drop(map);
        if done {
            if let Ok(mut s) = finishing().lock() {
                s.insert(id);
            }
            let _ = host.kill_mob(dim, id);
            if let Ok(mut s) = dying().lock() {
                s.remove(&(id as i32));
            }
            if let Ok(mut b) = brains().lock() {
                b.remove(&id);
            }
        }
        return;
    }

    let mut b = brain;
    let target = b.target.and_then(|t| resolve(host, dim, &t, &players));

    // `doHurtTarget`.
    if b.melee {
        b.melee = false;
        if let Some(t) = target {
            if logic::dist_sqr(pos, t.pos) <= 36.0 {
                let hurt = host.hurt_entity(dim, t.entity, ATTACK_DAMAGE, "minecraft:mob_attack", Some(id));
                push(host, dim, &t, melee_push(pos, t.pos));
                broadcast(host, Some(players.as_slice()), id, pos, Act::Swing, 0, rt.charged);
                super_trace(format_args!("creeper {id} hits {} : {hurt}", t.entity));
            }
        }
    }

    // `JumpAttackGoal.start` and `tick()`'s landing.
    if b.jump && !b.jumping {
        b.jump = false;
        if let (Some(t), Some(e)) = (target, host.entity(id)) {
            let v = jump_velocity(pos, t.pos);
            let _ = host.entity_set_velocity(e, v);
            let _ = host.play_sound("minecraft:entity.creeper.primed", pos, HOSTILE, 2.0, 0.6 + rt.rng.float() * 0.2);
            b.jumping = true;
            rt.jump_tick = 0;
            broadcast(host, Some(players.as_slice()), id, pos, Act::Jump, 0, rt.charged);
            super_trace(format_args!("creeper {id} jumps at {:?}", t.pos));
        }
    } else if b.jumping {
        rt.jump_tick += 1;
        let grounded = host.entity(id).and_then(|e| host.entity_on_ground(e)).unwrap_or(b.on_ground) || b.in_fluid;
        if (rt.jump_tick > 3 && grounded) || rt.jump_tick > 200 {
            b.jumping = false;
            let power = jump_power(rt.charged);
            blast(host, dim, pos, power);
            broadcast(host, Some(players.as_slice()), id, pos, Act::None, 0, rt.charged);
            super_trace(format_args!("creeper {id} lands: blast {power}"));
        }
    }

    // `ChargeAttackGoal`.
    let close = target.is_some_and(|t| logic::dist_sqr(pos, t.pos) < 25.0);
    if b.charge && !b.charging {
        b.charge = false;
        b.charging = true;
        rt.charge.time = 0;
        let thunder = host.weather().1 > 0.9;
        rt.charge.lightning = rt.rng.int(if thunder { 2 } else { 6 }) == 0 && !rt.charged;
        broadcast(host, Some(players.as_slice()), id, pos, Act::Charge, 0, rt.charged);
        super_trace(format_args!("creeper {id} charges (lightning {})", rt.charge.lightning));
    } else if b.charging {
        if rt.charge.time % 20 == 0 {
            let pitch = 0.7 + rt.rng.float() * 0.6;
            let _ = host.play_sound("minecraft:entity.creeper.primed", pos, HOSTILE, 0.6, pitch);
            let spark = ParticleOptions::simple(ParticleType::ElectricSpark);
            let _ = host.send_particles(&spark, None, [pos[0], pos[1] + 1.4, pos[2]], [0.8, 0.8, 0.8], 0.05, 10);
        }
        rt.charge.time += 1;
        if !rt.charge.continues(close) {
            let end = rt.charge.end(close);
            match end {
                ChargeEnd::Lightning => {
                    let _ = host.summon(dim, "minecraft:lightning_bolt", pos, Compound::new());
                    set_charged(host, id, rt, true);
                }
                ChargeEnd::Heal => {
                    let _ = host.set_mob_health(dim, id, (b.health + 30.0).min(MAX_HEALTH));
                    hearts(host, pos, 15);
                }
                ChargeEnd::Broken => {}
            }
            super_trace(format_args!("creeper {id} stops charging: {end:?}"));
            rt.charge = Charge::new(4 + rt.rng.int(3) as i32);
            b.charging = false;
            broadcast(host, Some(players.as_slice()), id, pos, Act::None, 0, rt.charged);
        }
    } else {
        b.charge = false;
    }
    drop(map);
    with(id, |w| {
        w.jump = b.jump;
        w.charge = b.charge;
        w.melee = b.melee;
        w.jumping = b.jumping;
        w.charging = b.charging;
    });
}

/// `tickDeath`: everything within 1.5 times the blast's power is drawn in;
/// at 100 ticks it blows up. True when it is time to die for good.
fn tick_death(host: &mut dyn Host, dim: &str, id: u32, pos: [f64; 3], rt: &mut Runtime, players: &[NativeServerPlayer]) -> bool {
    rt.death_time += 1;
    let power = death_power(rt.charged);
    let r = f64::from(power) * 1.5;
    for p in players.iter().filter(|p| p.dimension == dim && matches!(p.game_mode, 0 | 2) && p.entity_id > 0) {
        if (p.position[0] - pos[0]).abs() <= r + WIDTH && (p.position[1] - pos[1]).abs() <= r + HEIGHT && (p.position[2] - pos[2]).abs() <= r + WIDTH {
            let v = death_pull(pos, p.position, rt.death_time);
            let msg = PushMessage([v[0] as f32, v[1] as f32, v[2] as f32]);
            let _ = host.net_send_to_player(p.uuid, crate::net::PUSH, &msg.encode());
        }
    }
    let min = [pos[0] - r - WIDTH, pos[1] - r, pos[2] - r - WIDTH];
    let max = [pos[0] + r + WIDTH, pos[1] + r + HEIGHT, pos[2] + r + WIDTH];
    for other in host.entities_in(dim, min, max) {
        if other == id || players.iter().any(|p| p.entity_id == other as i32) {
            continue;
        }
        let Some(e) = host.entity(other) else { continue };
        if host.living(e).is_none() {
            continue;
        }
        if let Some(at) = host.entity_position(e) {
            let _ = host.push_mob(dim, other, death_pull(pos, at, rt.death_time));
        }
    }
    if rt.death_time % 20 == 0 {
        broadcast(host, Some(players), id, pos, Act::Death, rt.death_time, rt.charged);
    }
    if rt.death_time < MAX_DEATH_TIME {
        return false;
    }
    blast(host, dim, pos, power);
    if rt.killed_by_player {
        // `Enemy.XP_REWARD_HUGE` (50): the zombie class it copies gives 5.
        let mut orb = Compound::new();
        orb.insert("Value".into(), Tag::Short(45));
        let _ = host.summon(dim, "minecraft:experience_orb", [pos[0], pos[1] + 0.5, pos[2]], orb);
    }
    super_trace(format_args!("creeper {id} blows up: power {power}"));
    true
}

/// `MutatedExplosionHelper.explode` in place: a vanilla creeper with no fuse
/// and the blast's radius, which explodes on its first tick.
fn blast(host: &mut dyn Host, dim: &str, pos: [f64; 3], power: f32) {
    let mut nbt = Compound::new();
    nbt.insert("Fuse".into(), Tag::Short(0));
    nbt.insert("ignited".into(), Tag::Byte(1));
    nbt.insert("ExplosionRadius".into(), Tag::Byte(power.round().clamp(1.0, 127.0) as i8));
    nbt.insert("Silent".into(), Tag::Byte(1));
    nbt.insert("Invulnerable".into(), Tag::Byte(1));
    nbt.insert("CustomName".into(), Tag::String("Mutant Creeper".into()));
    nbt.insert("PersistenceRequired".into(), Tag::Byte(1));
    let at = [pos[0], pos[1] + 0.5, pos[2]];
    let summoned = host.summon(dim, "minecraft:creeper", at, nbt);
    if let Some(b) = summoned {
        if let Ok(mut s) = blasters().lock() {
            if s.len() > 256 {
                s.clear();
            }
            s.insert(b as i32);
        }
    }
    super_trace(format_args!("blast {power} at {at:?}: {summoned:?}"));
}

fn hearts(host: &mut dyn Host, pos: [f64; 3], count: i32) {
    if count <= 0 {
        return;
    }
    let heart = ParticleOptions::simple(ParticleType::Heart);
    let _ = host.send_particles(&heart, None, [pos[0], pos[1] + 2.0, pos[2]], [1.0, 0.8, 1.0], 0.02, count.min(30));
}

#[derive(Clone, Copy, Debug)]
struct Resolved {
    entity: u32,
    player: Option<u128>,
    pos: [f64; 3],
}

fn resolve(host: &mut dyn Host, dim: &str, t: &TargetSnap, players: &[NativeServerPlayer]) -> Option<Resolved> {
    if let Some(mob) = t.mob {
        let pos = host.entity(mob).and_then(|e| host.entity_position(e))?;
        return Some(Resolved { entity: mob, player: None, pos });
    }
    players
        .iter()
        .filter(|p| p.dimension == dim && p.entity_id > 0 && p.health > 0.0 && matches!(p.game_mode, 0 | 2))
        .map(|p| (logic::dist_sqr(p.position, t.pos), p))
        .filter(|(d, _)| *d < 16.0)
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, p)| Resolved { entity: p.entity_id as u32, player: Some(p.uuid), pos: p.position })
}

fn push(host: &mut dyn Host, dim: &str, t: &Resolved, v: [f64; 3]) {
    match t.player {
        Some(uuid) => {
            let msg = PushMessage([v[0] as f32, v[1] as f32, v[2] as f32]);
            let _ = host.net_send_to_player(uuid, crate::net::PUSH, &msg.encode());
        }
        None => {
            let _ = host.push_mob(dim, t.entity, v);
        }
    }
}

fn broadcast(host: &mut dyn Host, players: Option<&[NativeServerPlayer]>, id: u32, pos: [f64; 3], act: Act, elapsed: u32, charged: bool) {
    let owned;
    let players = match players {
        Some(p) => p,
        None => {
            owned = host.server_players();
            &owned
        }
    };
    let msg = AnimMessage {
        entity: id,
        anim: act.id(),
        elapsed: elapsed.min(u32::from(u16::MAX)) as u16,
        flags: if charged { CHARGED } else { 0 },
    };
    let bytes = msg.encode();
    for p in players {
        if logic::dist_sqr(p.position, pos) <= VIEW * VIEW {
            let _ = host.net_send_to_player(p.uuid, CHANNEL, &bytes);
        }
    }
}

/// Forget creepers that stopped ticking (dead, unloaded or removed).
pub fn forget_stale(now: u64) {
    let mut gone = Vec::new();
    if let Ok(mut map) = runtimes().lock() {
        map.retain(|(_, id), rt| {
            let keep = now.saturating_sub(rt.last_seen) < 400;
            if !keep {
                gone.push(*id);
            }
            keep
        });
    }
    if let Ok(mut b) = brains().lock() {
        for id in &gone {
            b.remove(id);
        }
    }
    for set in [dying(), charged_set()] {
        if let Ok(mut s) = set.lock() {
            for id in &gone {
                s.remove(&(*id as i32));
            }
        }
    }
    if let Ok(mut h) = hits().lock() {
        for id in &gone {
            h.remove(&(*id as i32));
        }
    }
}

// --- client --------------------------------------------------------------------------------

/// `MutantCreeperModel.createBodyLayer(CubeDeformation.NONE)`, 128x64.
pub fn layer() -> LayerDefinition {
    let mut mesh = MeshDefinition::new();
    let root = mesh.root();
    let pose = |p: Part| PartPose::offset_and_rotation(p.x, p.y, p.z, p.xr, p.yr, p.zr);
    let pelvis = mesh.add_or_replace_child(
        root,
        "pelvis",
        CubeListBuilder::create().tex_offs(0, 0).add_box([-5.0, -14.0, -4.0], [10.0, 14.0, 8.0]),
        pose(PELVIS),
    );
    let body = mesh.add_or_replace_child(
        pelvis,
        "body",
        CubeListBuilder::create().tex_offs(36, 0).add_box([-4.5, -14.0, -3.5], [9.0, 16.0, 7.0]),
        pose(BODY),
    );
    let neck = mesh.add_or_replace_child(
        body,
        "neck",
        CubeListBuilder::create().tex_offs(68, 0).add_box([-4.0, -14.0, -3.0], [8.0, 14.0, 6.0]),
        pose(NECK),
    );
    mesh.add_or_replace_child(
        neck,
        "head",
        CubeListBuilder::create().tex_offs(0, 22).add_box([-5.0, -12.0, -5.0], [10.0, 12.0, 10.0]),
        pose(HEAD),
    );
    let rf = mesh.add_or_replace_child(
        pelvis,
        "right_front_leg",
        CubeListBuilder::create().tex_offs(40, 24).mirror().add_box([-3.0, -4.0, -14.0], [6.0, 4.0, 14.0]),
        pose(RIGHT_FRONT_LEG),
    );
    mesh.add_or_replace_child(
        rf,
        "right_front_leg_lower",
        CubeListBuilder::create().tex_offs(96, 0).mirror().add_box([-3.5, 0.0, -4.0], [7.0, 20.0, 8.0]),
        pose(RIGHT_FRONT_LEG_LOWER),
    );
    let lf = mesh.add_or_replace_child(
        pelvis,
        "left_front_leg",
        CubeListBuilder::create().tex_offs(40, 24).add_box([-3.0, -4.0, -14.0], [6.0, 4.0, 14.0]),
        pose(LEFT_FRONT_LEG),
    );
    mesh.add_or_replace_child(
        lf,
        "left_front_leg_lower",
        CubeListBuilder::create().tex_offs(96, 0).add_box([-3.5, 0.0, -4.0], [7.0, 20.0, 8.0]),
        pose(LEFT_FRONT_LEG_LOWER),
    );
    let rh = mesh.add_or_replace_child(
        pelvis,
        "right_hind_leg",
        CubeListBuilder::create().tex_offs(0, 44).mirror().add_box([-2.0, -4.0, 0.0], [4.0, 4.0, 14.0]),
        pose(RIGHT_HIND_LEG),
    );
    mesh.add_or_replace_child(
        rh,
        "right_hind_leg_lower",
        CubeListBuilder::create().tex_offs(80, 28).mirror().add_box([-3.0, 0.0, -3.0], [6.0, 18.0, 6.0]),
        pose(HIND_LEG_LOWER),
    );
    let lh = mesh.add_or_replace_child(
        pelvis,
        "left_hind_leg",
        CubeListBuilder::create().tex_offs(0, 44).add_box([-2.0, -4.0, 0.0], [4.0, 4.0, 14.0]),
        pose(LEFT_HIND_LEG),
    );
    mesh.add_or_replace_child(
        lh,
        "left_hind_leg_lower",
        CubeListBuilder::create().tex_offs(80, 28).add_box([-3.0, 0.0, -3.0], [6.0, 18.0, 6.0]),
        pose(HIND_LEG_LOWER),
    );
    LayerDefinition::create(mesh, 128, 64)
}

/// One part's place and turn.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Part {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub xr: f32,
    pub yr: f32,
    pub zr: f32,
}

const fn part(x: f32, y: f32, z: f32, xr: f32, yr: f32, zr: f32) -> Part {
    Part { x, y, z, xr, yr, zr }
}

pub const PELVIS: Part = part(0.0, 14.0, -3.0, -PI / 4.0, 0.0, 0.0);
pub const BODY: Part = part(0.0, -12.0, 0.0, PI * 3.0 / 10.0, 0.0, 0.0);
pub const NECK: Part = part(0.0, -11.0, 1.0, PI / 3.0, 0.0, 0.0);
pub const HEAD: Part = part(0.0, -12.0, 1.0, PI / 6.0, 0.0, 0.0);
pub const RIGHT_FRONT_LEG: Part = part(-3.0, 0.0, 0.0, PI / 10.0, PI / 4.0, 0.0);
pub const RIGHT_FRONT_LEG_LOWER: Part = part(0.0, -4.0, -14.0, -PI / 15.0, -PI / 8.0, 0.0);
pub const LEFT_FRONT_LEG: Part = part(3.0, 0.0, 0.0, PI / 10.0, -PI / 4.0, 0.0);
pub const LEFT_FRONT_LEG_LOWER: Part = part(0.0, -4.0, -14.0, -PI / 15.0, PI / 8.0, 0.0);
pub const RIGHT_HIND_LEG: Part = part(-2.0, -2.0, 4.0, PI / 3.0, -PI / 5.0, 0.0);
pub const LEFT_HIND_LEG: Part = part(2.0, -2.0, 4.0, PI / 3.0, PI / 5.0, 0.0);
pub const HIND_LEG_LOWER: Part = part(0.0, -4.0, 14.0, 2.0 * PI / 13.0, 0.0, 0.0);

/// The parts `setupAnim` moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rig {
    pub pelvis: Part,
    pub body: Part,
    pub neck: Part,
    pub head: Part,
    pub right_front_leg: Part,
    pub left_front_leg: Part,
    pub right_hind_leg: Part,
    pub left_hind_leg: Part,
}

impl Default for Rig {
    fn default() -> Self {
        Rig {
            pelvis: PELVIS,
            body: BODY,
            neck: NECK,
            head: HEAD,
            right_front_leg: RIGHT_FRONT_LEG,
            left_front_leg: LEFT_FRONT_LEG,
            right_hind_leg: RIGHT_HIND_LEG,
            left_hind_leg: LEFT_HIND_LEG,
        }
    }
}

impl Rig {
    pub fn parts(&self) -> [(&'static [&'static str], Part); 8] {
        [
            (&["pelvis"], self.pelvis),
            (&["pelvis", "body"], self.body),
            (&["pelvis", "body", "neck"], self.neck),
            (&["pelvis", "body", "neck", "head"], self.head),
            (&["pelvis", "right_front_leg"], self.right_front_leg),
            (&["pelvis", "left_front_leg"], self.left_front_leg),
            (&["pelvis", "right_hind_leg"], self.right_hind_leg),
            (&["pelvis", "left_hind_leg"], self.left_hind_leg),
        ]
    }
}

/// `MutantCreeperModel.setupAnim`. `attack` is the swing (`attackTime`, 0
/// to 1, 0 when not swinging); yaw and pitch in degrees.
pub fn setup_anim(walk_position: f32, walk_speed: f32, age: f32, head_yaw: f32, pitch: f32, attack: f32) -> Rig {
    let mut r = Rig::default();
    let pos = walk_position;
    let speed = walk_speed;
    let breathe = (age * 0.1).sin();
    let left_front = (((pos * PI / 4.0).sin() + 0.4) * speed).max(0.0);
    let right_front = (((pos * PI / 4.0 + PI).sin() + 0.4) * speed).max(0.0);
    let front_sway = (pos * PI / 8.0).sin() * speed;
    let right_hind = (((pos * PI / 4.0 + PI / 2.0).sin() + 0.4) * speed).max(0.0);
    let left_hind = (((pos * PI / 4.0 + PI * 1.5).sin() + 0.4) * speed).max(0.0);
    let hind_sway = (pos * PI / 8.0 + PI / 2.0).sin() * speed;
    let face_yaw = head_yaw.to_radians();
    let face_pitch = pitch.to_radians();
    r.pelvis.y += (pos * PI / 4.0).sin() * speed * 0.5;
    r.body.xr += breathe * 0.02 + face_pitch / 3.0;
    r.body.yr += face_yaw / 3.0;
    r.neck.xr += breathe * 0.02 + face_pitch / 3.0;
    r.neck.yr = face_yaw / 3.0;
    r.head.xr += breathe * 0.02 + face_pitch / 3.0;
    r.head.yr = face_yaw / 3.0;
    r.left_front_leg.xr -= left_front * 0.3;
    r.left_front_leg.yr += front_sway * 0.2;
    r.left_front_leg.zr += front_sway * 0.2;
    r.right_front_leg.xr -= right_front * 0.3;
    r.right_front_leg.yr -= front_sway * 0.2;
    r.right_front_leg.zr -= front_sway * 0.2;
    r.left_hind_leg.xr += left_hind * 0.3;
    r.left_hind_leg.yr -= hind_sway * 0.2;
    r.left_hind_leg.zr -= hind_sway * 0.2;
    r.right_hind_leg.xr += right_hind * 0.3;
    r.right_hind_leg.yr += hind_sway * 0.2;
    r.right_hind_leg.zr += hind_sway * 0.2;
    if attack > 0.0 {
        let swing = (attack * PI).sin();
        r.body.xr += swing * PI / 3.0;
        r.neck.xr -= swing * PI / 4.0;
    }
    r
}

/// What one creeper is doing, as this client heard it.
#[derive(Clone, Copy, Debug)]
struct Seen {
    act: Act,
    start: u32,
    charged: bool,
}

fn seen() -> &'static Mutex<HashMap<u32, Seen>> {
    static S: OnceLock<Mutex<HashMap<u32, Seen>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn baked() -> &'static Mutex<Option<ModelPart>> {
    static M: OnceLock<Mutex<Option<ModelPart>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(None))
}

/// A message on `CHANNEL`, at client tick `now`.
pub fn heard(data: &[u8], now: u32) {
    let Some(msg) = AnimMessage::decode(data) else { return };
    let Some(act) = Act::from_id(msg.anim) else { return };
    let start = now.saturating_sub(u32::from(msg.elapsed));
    let charged = msg.flags & CHARGED != 0;
    if let Ok(mut m) = seen().lock() {
        let e = m.entry(msg.entity).or_insert(Seen { act, start, charged });
        if e.act != act || e.start.abs_diff(start) > 3 {
            e.act = act;
            e.start = start;
        }
        e.charged = charged;
    }
}

pub fn client_tick(now: u32) {
    if now % 200 == 0 {
        if let Ok(mut m) = seen().lock() {
            // Keep what is known of each creeper (its charge) a while; drop a
            // stale action.
            for s in m.values_mut() {
                if s.act != Act::None && now.saturating_sub(s.start) > 600 {
                    s.act = Act::None;
                }
            }
            if m.len() > 512 {
                m.clear();
            }
        }
    }
}

fn part_mut<'a>(root: &'a mut ModelPart, path: &[&str]) -> Option<&'a mut ModelPart> {
    let mut at = root;
    for name in path {
        at = at.get_child_mut(name)?;
    }
    Some(at)
}

/// `MutantCreeperRenderer`: draw it with its own model. `now` is the client
/// tick with the partial tick.
pub fn submit(host: &dyn Host, mob: &NativeMobRender<'_>, draw: &mut BlockEntityDraw, now: f32) -> Flow {
    let model = {
        let Ok(mut cache) = baked().lock() else { return Flow::Continue };
        if cache.is_none() {
            *cache = host.bake_layer(LAYER);
        }
        cache.clone()
    };
    let Some(mut model) = model else { return Flow::Continue };
    let state = seen().lock().ok().and_then(|m| m.get(&mob.id).copied());
    let since = |s: &Seen| (now - s.start as f32).max(0.0);
    let (act, t, charged) = state.map(|s| (s.act, since(&s), s.charged)).unwrap_or((Act::None, 0.0, false));
    let attack = if act == Act::Swing && t < SWING_TIME as f32 { t / SWING_TIME as f32 } else { 0.0 };
    let death_time = if act == Act::Death { t.min(MAX_DEATH_TIME as f32) } else { 0.0 };
    let rig = setup_anim(mob.walk_position, mob.walk_speed, mob.age, mob.head_yaw, mob.pitch, attack);
    model.reset_all_poses();
    for (path, p) in rig.parts() {
        if let Some(part) = part_mut(&mut model, path) {
            part.x = p.x;
            part.y = p.y;
            part.z = p.z;
            part.x_rot = p.xr;
            part.y_rot = p.yr;
            part.z_rot = p.zr;
        }
    }
    draw.push();
    if death_time > 0.0 {
        // `tickDeath`'s shaking (`setPosRaw` by up to 0.1 each way).
        let k = (now * 7.3).sin() * 0.1;
        let j = (now * 5.1).cos() * 0.1;
        draw.translate([k, 0.0, j]);
    }
    draw.scale([1.0, -1.0, -1.0]);
    let s = render_scale(death_time);
    draw.scale([s, s, s]);
    draw.translate([0.0, -1.501, 0.0]);
    // The white flash of `getWhiteOverlayProgress` can't be drawn by a tint
    // (it only darkens): a jump, a charge and the death pulse darker instead;
    // a charged creeper is tinted blue (in place of `PowerableLayer`).
    let pulse = match act {
        Act::Jump => (t as u32 / 2) % 2 == 1,
        Act::Charge => (t % 20.0) < 10.0,
        Act::Death => (t as u32 / (5 - (death_time as u32 * 4 / MAX_DEATH_TIME).min(3))) % 2 == 1,
        _ => false,
    };
    let mut tint: u32 = if charged { 0xA8C8FF } else { 0xFFFFFF };
    if pulse {
        tint = darken(tint);
    }
    if mob.hurt_time > 0 {
        tint &= 0xFF9999;
    }
    host.draw_model_part(draw, &model, TEXTURE, tint);
    draw.pop();
    Flow::Cancel
}

fn darken(rgb: u32) -> u32 {
    let c = |shift: u32| (((rgb >> shift) & 0xFF) * 3 / 4) << shift;
    c(16) | c(8) | c(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acts_round_trip() {
        for a in [Act::None, Act::Swing, Act::Jump, Act::Charge, Act::Death] {
            assert_eq!(Act::from_id(a.id()), Some(a));
        }
        assert_eq!(Act::from_id(7), None);
    }

    #[test]
    fn charge_needs_a_wound_and_distance() {
        let mut rng = Rng::new(5);
        // Unhurt: never.
        assert!(!(0..10_000).any(|_| charge_choice(&mut rng, MAX_HEALTH, 100.0, false)));
        // Hurt by a sixth, target 10 blocks off: sometimes.
        let n = (0..100_000).filter(|_| charge_choice(&mut rng, 90.0, 100.0, false)).count();
        assert!((400..1000).contains(&n), "{n}");
        // Too close or too far: never.
        assert!(!(0..10_000).any(|_| charge_choice(&mut rng, 10.0, 4.0, false)));
        assert!(!(0..10_000).any(|_| charge_choice(&mut rng, 10.0, 2000.0, false)));
    }

    #[test]
    fn jump_only_from_the_ground() {
        let mut rng = Rng::new(9);
        assert!(!(0..10_000).any(|_| jump_choice(&mut rng, false, 10.0, false)));
        assert!(!(0..10_000).any(|_| jump_choice(&mut rng, true, 10.0, true)));
        let n = (0..100_000).filter(|_| jump_choice(&mut rng, true, 10.0, false)).count();
        assert!((600..1200).contains(&n), "{n}");
        let v = jump_velocity([0.0, 64.0, 0.0], [10.0, 64.0, -5.0]);
        assert!((v[0] - 2.0).abs() < 1e-9 && v[1] == 1.4 && (v[2] + 1.0).abs() < 1e-9);
    }

    #[test]
    fn charge_ends() {
        let mut c = Charge::new(3);
        assert!(c.continues(false));
        c.hits = 0;
        assert!(!c.continues(false));
        assert_eq!(c.end(false), ChargeEnd::Broken);
        let mut c = Charge::new(3);
        c.time = MAX_CHARGE_TIME;
        assert!(!c.continues(false));
        assert_eq!(c.end(false), ChargeEnd::Heal);
        let mut c = Charge::new(3);
        c.lightning = true;
        assert!(c.continues(false));
        assert!(!c.continues(true));
        assert_eq!(c.end(true), ChargeEnd::Lightning);
    }

    #[test]
    fn death_pulls_in_harder_at_the_end() {
        let early = death_pull([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], 10);
        let late = death_pull([0.0, 0.0, 0.0], [10.0, 0.0, 0.0], 100);
        assert!(early[0] < 0.0 && late[0] < early[0]);
        assert!((late[0] + 0.09).abs() < 1e-9);
        assert_eq!(death_pull([1.0, 2.0, 3.0], [1.0, 2.0, 3.0], 50), [0.0, 0.0, 0.0]);
        assert!((render_scale(0.0) - 1.2).abs() < 1e-6 && (render_scale(100.0) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn blasts_and_damage_types() {
        assert_eq!(jump_power(false), 4.0);
        assert_eq!(death_power(true), 12.0);
        assert!(is_explosion("minecraft:explosion") && !is_explosion("minecraft:generic"));
        assert!(witch_resistant("minecraft:magic") && !witch_resistant("minecraft:mob_attack"));
        let p = melee_push([0.0, 0.0, 0.0], [3.0, 0.0, 4.0]);
        assert!((p[0] - 0.3).abs() < 1e-9 && (p[1] - 0.15).abs() < 1e-9);
    }

    #[test]
    fn standing_still_is_the_rest_pose_but_for_breathing() {
        let r = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(r, Rig::default());
        let swing = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, 0.5);
        assert!(swing.body.xr > BODY.xr && swing.neck.xr < NECK.xr);
    }
}
