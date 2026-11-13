//! The Mutant Skeleton: `MutantSkeleton`, `MutantArrow`, `MutantSkeletonModel`
//! and `MutantSkeletonRenderer`, translated from Mutant Monsters (AGPL-3.0,
//! see NOTICE.md) onto Horizon. One file holds its pure logic (tested), its
//! goals, its server half and its client half, as `creeper.rs` does for the
//! Mutant Creeper.
//!
//! Differences forced by Horizon (see README.md, "Not done"):
//! - Horizon lets a mod type copy a zombie, not a skeleton: the Mutant
//!   Skeleton is a silent zombie-class mob that plays vanilla skeleton and
//!   crossbow sounds (upstream's own sounds are All Rights Reserved).
//! - `MutantArrow` is a vanilla arrow summoned with no gravity, a critical
//!   flag and the skeleton as its owner; the ones that hit nothing are removed
//!   after 200 ticks here. An arrow carries one effect at most (a tipped
//!   arrow's), so upstream's hunger is left out.
//! - `MutantSkeletonBodyPart` (the bones that fly off when it dies) is not an
//!   entity here: the eighteen parts are thrown as their items.
//! - The crossbow in its hand (`MutantSkeletonCrossbowLayer`) is not drawn.
use crate::brain::TargetSnap;
use crate::logic::{self, Rng};
use crate::net::{AnimMessage, PushMessage};
use horizon_client::mobs::{
    Mob, Species, Who,
    goals::{Cx, Float, Goal, HurtByTarget, LOOK, LookAtPlayer, MOVE, NearestTarget, RandomLookAround, Stroll},
};
use horizon_client::mod_api::{
    BlockEntityDraw, Compound, CubeDeformation, CubeListBuilder, Flow, Host, LayerDefinition, MeshDefinition,
    ModelPart, NativeMobRender, NativeServerPlayer, PartId, PartPose, Tag,
};
use std::{
    collections::{HashMap, HashSet},
    f32::consts::PI,
    sync::{Arc, Mutex, OnceLock},
};

pub const MUTANT_SKELETON: &str = "mutant_monsters:mutant_skeleton";
/// `MutantSkeleton.createAttributes`.
pub const MAX_HEALTH: f32 = 150.0;
pub const ATTACK_DAMAGE: f32 = 3.0;
/// `EntityType.Builder.sized(1.2F, 3.6F).eyeHeight(3.25F)`.
pub const WIDTH: f64 = 1.2;
pub const HEIGHT: f64 = 3.6;
pub const EYE_HEIGHT: f64 = 3.25;
/// `MutantArrow.tick`: gone after 200 ticks.
pub const ARROW_LIFE: u64 = 200;

/// Server to client: what a Mutant Skeleton is doing (an `AnimMessage` with
/// `Anim` ids), in place of `ClientboundAnimationMessage`.
pub const CHANNEL: &str = "mutant_monsters:skeleton";
pub const LAYER: &str = "mutant_monsters:mutant_skeleton#main";
pub const TEXTURE: &str = "mutant_monsters:entity/mutant_skeleton";

const HOSTILE: usize = 5;
const VIEW: f64 = 96.0;

/// `MutantSkeleton.MELEE_ANIMATION` and the three others.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Anim {
    #[default]
    None,
    Melee,
    ConstrictRibs,
    Shoot,
    MultiShot,
}

impl Anim {
    pub fn id(self) -> u8 {
        match self {
            Anim::None => 0,
            Anim::Melee => 1,
            Anim::ConstrictRibs => 2,
            Anim::Shoot => 3,
            Anim::MultiShot => 4,
        }
    }

    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            0 => Anim::None,
            1 => Anim::Melee,
            2 => Anim::ConstrictRibs,
            3 => Anim::Shoot,
            4 => Anim::MultiShot,
            _ => return None,
        })
    }

    /// `EntityAnimation.duration`.
    pub fn duration(self) -> u32 {
        match self {
            Anim::None => 0,
            Anim::Melee => 14,
            Anim::ConstrictRibs => 20,
            Anim::Shoot => 32,
            Anim::MultiShot => 30,
        }
    }
}

// --- pure logic -------------------------------------------------------------------

/// `doHurtTarget`: three times in four a swing, else the ribs.
pub fn melee_choice(rng: &mut Rng) -> Anim {
    if rng.int(4) != 0 { Anim::Melee } else { Anim::ConstrictRibs }
}

/// `ShootGoal.canUse`: one time in twelve, the target more than 2 blocks off
/// and in sight.
pub fn shoot_choice(rng: &mut Rng, dist_sqr: f64, sees: bool) -> bool {
    rng.int(12) == 0 && dist_sqr > 4.0 && sees
}

/// `MultiShotGoal.canUse`: every third tick, on the ground, one time in
/// twenty-six, the target in sight.
pub fn multi_shot_choice(rng: &mut Rng, age: u32, on_ground: bool, sees: bool) -> bool {
    age % 3 == 0 && on_ground && rng.int(26) == 0 && sees
}

/// `MultiShotGoal.tick` at tick 10: the leap, towards a far target and away
/// from one within 4 blocks.
pub fn multi_shot_leap(rng: &mut Rng, skeleton: [f64; 3], target: [f64; 3]) -> [f64; 3] {
    let mut x = target[0] - skeleton[0];
    let mut z = target[2] - skeleton[2];
    let mut scale = f64::from(0.06 + rng.float() * 0.03);
    if logic::dist_sqr(skeleton, target) < 16.0 {
        x = -x;
        z = -z;
        scale *= 5.0;
    }
    [x * scale, 1.1, z * scale]
}

/// `MutantArrow.shoot(target, velocity, randomization)`: the arrow's motion
/// from `from` at a point around `aim`.
pub fn arrow_motion(rng: &mut Rng, from: [f64; 3], aim: [f64; 3], velocity: f32, randomization: f32) -> [f64; 3] {
    let mut jitter = || f64::from((rng.float() - 0.5) * randomization * 2.0);
    let at = [aim[0] + jitter(), aim[1] + jitter(), aim[2] + jitter()];
    let d = [at[0] - from[0], at[1] - from[1], at[2] - from[2]];
    let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if len < 1e-6 {
        return [0.0, 0.0, 0.0];
    }
    let v = f64::from(velocity);
    [d[0] / len * v, d[1] / len * v, d[2] / len * v]
}

/// `EntityUtil.getHeadAngle`: how far (degrees) `other` is from where the
/// skeleton's head points.
pub fn head_angle(skeleton: [f64; 3], head_yaw: f32, other: [f64; 3]) -> f32 {
    let x = skeleton[0] - other[0];
    let z = skeleton[2] - other[2];
    let to = (z.atan2(x) as f32).to_degrees() + 90.0;
    let mut diff = (head_yaw - to) % 360.0;
    if diff >= 180.0 {
        diff -= 360.0;
    }
    if diff < -180.0 {
        diff += 360.0;
    }
    diff.abs()
}

/// `MeleeGoal.tick` at tick 3: what the swing does to something `other`
/// blocks away, if it reaches it (within 3 blocks and 60 degrees of the
/// head): the push it gets. `vy` is its own upward speed.
pub fn melee_push(rng: &mut Rng, skeleton: [f64; 3], head_yaw: f32, other: [f64; 3], vy: f64) -> Option<[f64; 3]> {
    let dist = logic::dist_sqr(skeleton, other).sqrt();
    if dist > 3.0 || dist < 1e-6 || head_angle(skeleton, head_yaw, other) >= 60.0 {
        return None;
    }
    let x = skeleton[0] - other[0];
    let z = skeleton[2] - other[2];
    let power = f64::from(1.8 + rng.int(5) as f32 * 0.15);
    Some([-x / dist * power, vy.max(0.280_000_001_192_092_9), -z / dist * power])
}

/// `ConstrictRibsGoal.tick` at tick 6: where the target is flung.
pub fn constrict_push(rng: &mut Rng) -> [f64; 3] {
    let side = |rng: &mut Rng| f64::from(1.0 + rng.float() * 0.4) * if rng.bool() { 1.0 } else { -1.0 };
    let x = side(rng);
    let y = f64::from(0.4 + rng.float() * 0.8);
    let z = side(rng);
    [x, y, z]
}

/// `MutantSkeletonBodyPart.BodyPart.values()` by `BodyPartGroup`: the items
/// its eighteen parts leave.
pub const BODY_PARTS: &[(&str, u8)] = &[
    ("mutant_monsters:mutant_skeleton_pelvis", 1),
    ("mutant_monsters:mutant_skeleton_rib", 6),
    ("mutant_monsters:mutant_skeleton_skull", 1),
    ("mutant_monsters:mutant_skeleton_limb", 8),
    ("mutant_monsters:mutant_skeleton_shoulder_pad", 2),
];

/// A UUID as the four ints of its NBT form.
pub fn uuid_ints(uuid: u128) -> [i32; 4] {
    [(uuid >> 96) as u32 as i32, (uuid >> 64) as u32 as i32, (uuid >> 32) as u32 as i32, uuid as u32 as i32]
}

// --- what the goals and the server share --------------------------------------------------

#[derive(Clone, Debug, Default)]
struct Brain {
    pos: [f64; 3],
    on_ground: bool,
    health: f32,
    age: u32,
    head_yaw: f32,
    hurt_time: i32,
    sees: bool,
    target: Option<TargetSnap>,
    /// Asked by the goals, started by the server.
    request: Option<Anim>,
    /// Set by the server.
    anim: Anim,
    anim_tick: u32,
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

/// The Mutant Skeletons the server ticks, read by the damage hook (one never
/// hurts another, nor itself with its arrows).
fn ids() -> &'static Mutex<HashSet<i32>> {
    static S: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashSet::new()))
}

// --- goals ------------------------------------------------------------------------------

/// `MutantSkeleton.registerGoals`, with its size. Asked for every new mob.
pub fn register(mob: &mut Mob) {
    if mob.kind.id != MUTANT_SKELETON {
        return;
    }
    let kind = Arc::make_mut(&mut mob.kind);
    kind.width = WIDTH;
    kind.height = HEIGHT;
    kind.eye_height = EYE_HEIGHT;
    kind.flags.burns_in_day = false;

    let seed = u64::from(mob.id).wrapping_mul(0xD6E8_FEB8_6659_FD93);
    let goals = &mut mob.ai.goals;
    goals.clear();
    goals.add(0, Float);
    goals.add(0, Sense { rng: Rng::new(seed ^ 5) });
    goals.add(0, Busy);
    goals.add(1, Melee { speed: 1.1, recalc: 0, cooldown: 0, rng: Rng::new(seed ^ 6) });
    goals.add(3, Stroll::avoiding_water(1.0));
    goals.add(4, LookAtPlayer::new(8.0));
    goals.add(5, RandomLookAround::default());

    let targets = &mut mob.ai.targets;
    targets.clear();
    // HurtByNearestTargetGoal(this, WitherBoss.class).
    targets.add(0, HurtByTarget::new().ignoring(&[Species::Wither]));
    targets.add(1, NearestTarget::player(true).unseen_memory(300));
    targets.add(2, NearestTarget::species(&[Species::IronGolem], true));
    targets.add(3, NearestTarget::species(&[Species::Wolf], true));
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

/// Writes the mob's state for the server half, and makes the shoot and
/// multi-shot decisions (`ShootGoal.canUse`, `MultiShotGoal.canUse`).
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
        let sees = match (target, m.ai.target) {
            (Some(_), Some(who)) => m.sees(cx.w, who),
            _ => false,
        };
        let pos = [m.position.x, m.position.y, m.position.z];
        let (on_ground, health, age, head_yaw, hurt_time) = (m.on_ground, m.health, m.age, m.head_yaw, m.hurt_time);
        let rng = &mut self.rng;
        with(m.id, |b| {
            b.pos = pos;
            b.on_ground = on_ground;
            b.health = health;
            b.age = age;
            b.head_yaw = head_yaw;
            b.hurt_time = hurt_time;
            b.sees = sees;
            b.target = target;
            let Some(t) = target else { return };
            // `Mob.serverAiStep` asks idle goals every other tick.
            if b.anim != Anim::None || b.request.is_some() || age % 2 != 0 {
                return;
            }
            if shoot_choice(rng, logic::dist_sqr(pos, t.pos), sees) {
                b.request = Some(Anim::Shoot);
            } else if multi_shot_choice(rng, age, on_ground, sees) {
                b.request = Some(Anim::MultiShot);
            }
        });
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

/// The four `AnimationGoal`s' hold on the skeleton (MOVE and LOOK): it stands
/// and looks at its target while an attack plays.
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
        get(m.id).is_some_and(|b| b.anim != Anim::None)
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.nav_stop();
        let Some(b) = get(m.id) else { return };
        // ConstrictRibsGoal looks at the target until tick 6, the others
        // throughout.
        if b.anim == Anim::ConstrictRibs && b.anim_tick >= 6 {
            return;
        }
        if let Some(t) = snapshot(m, cx).filter(|t| t.alive) {
            m.look_at_speed([t.pos[0], t.eye_y, t.pos[2]], 30.0, 30.0);
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

/// `MutantMeleeAttackGoal(this, 1.1).setMaxAttackTick(5)`: walk at the target
/// and, in reach, start what `doHurtTarget` starts.
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
        if get(m.id).is_some_and(|b| b.anim != Anim::None) {
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
            self.cooldown = 5;
            let rng = &mut self.rng;
            with(m.id, |b| {
                if b.anim == Anim::None && b.request.is_none() {
                    b.request = Some(melee_choice(rng));
                }
            });
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

// --- server -------------------------------------------------------------------------------

struct Runtime {
    last_health: f32,
    /// `Mob.ambientSoundTime`.
    ambient: i32,
    /// The arrows it shot that are still flying, with the tick they left.
    arrows: Vec<(u32, u64)>,
    rng: Rng,
    last_seen: u64,
}

fn runtimes() -> &'static Mutex<HashMap<(String, u32), Runtime>> {
    static RT: OnceLock<Mutex<HashMap<(String, u32), Runtime>>> = OnceLock::new();
    RT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn trace(args: std::fmt::Arguments<'_>) {
    crate::server::trace_line(args);
}

/// `MutantSkeleton.hurtServer`: nothing a Mutant Skeleton does hurts one.
pub fn living_incoming_damage(entity: i32, causing: i32) -> Flow {
    let own = causing > 0 && causing != entity && ids().lock().map(|s| s.contains(&causing)).unwrap_or(false);
    let itself = causing == entity;
    if own || itself { Flow::Cancel } else { Flow::Continue }
}

/// `MutantSkeleton.die`: it shatters. Everything living close by takes 7
/// damage and its eighteen parts fly off (as their items here).
pub fn living_death(host: &mut dyn Host, entity: u32, attacker: u32) -> Flow {
    let dim = runtimes().lock().ok().and_then(|m| m.keys().find(|(_, i)| *i == entity).map(|(d, _)| d.clone()));
    let pos = host.entity(entity).and_then(|e| host.entity_position(e));
    let (Some(dim), Some(pos)) = (dim, pos) else { return Flow::Continue };
    let players = host.server_players();
    let by_player = attacker > 0 && players.iter().any(|p| p.entity_id == attacker as i32);
    let mut rng = Rng::new(u64::from(entity) ^ 0x5E1E ^ (pos[0].to_bits() >> 7));
    // Forget it first: the shatter must not come back here through a hook.
    if let Ok(mut m) = runtimes().lock() {
        m.remove(&(dim.clone(), entity));
    }
    if let Ok(mut b) = brains().lock() {
        b.remove(&entity);
    }
    if let Ok(mut s) = ids().lock() {
        s.remove(&(entity as i32));
    }

    let half = WIDTH / 2.0;
    let min = [pos[0] - half - 3.0, pos[1] - 2.0, pos[2] - half - 3.0];
    let max = [pos[0] + half + 3.0, pos[1] + HEIGHT + 2.0, pos[2] + half + 3.0];
    let inside = |at: [f64; 3]| (0..3).all(|i| at[i] >= min[i] - 0.3 && at[i] <= max[i] + if i == 1 { 0.0 } else { 0.3 });
    let shatter = "mutant_monsters:mutant_skeleton_shatter";
    let mut hurt = 0;
    for p in players.iter().filter(|p| p.dimension == dim && p.entity_id > 0 && matches!(p.game_mode, 0 | 2)) {
        if inside(p.position) {
            // The dying skeleton may no longer stand as an attacker: then the blow has none.
            if host.hurt_entity(&dim, p.entity_id as u32, 7.0, shatter, Some(entity))
                || host.hurt_entity(&dim, p.entity_id as u32, 7.0, shatter, None)
            {
                hurt += 1;
            }
        }
    }
    for other in host.entities_in(&dim, min, max) {
        if other == entity || players.iter().any(|p| p.entity_id == other as i32) {
            continue;
        }
        let Some(e) = host.entity(other) else { continue };
        if host.living(e).is_none() {
            continue;
        }
        if host.hurt_entity(&dim, other, 7.0, shatter, Some(entity)) || host.hurt_entity(&dim, other, 7.0, shatter, None) {
            hurt += 1;
        }
    }

    let mut thrown = 0;
    for (item, count) in BODY_PARTS {
        for _ in 0..*count {
            let v = [
                f64::from(rng.float() * 0.8 * 2.0 - 0.8),
                f64::from(rng.float() * 0.25 + 0.1),
                f64::from(rng.float() * 0.8 * 2.0 - 0.8),
            ];
            let at = [pos[0], pos[1] + 0.6 + f64::from(rng.float()) * (HEIGHT - 1.0), pos[2]];
            let mut stack = Compound::new();
            stack.insert("id".into(), Tag::String((*item).into()));
            stack.insert("count".into(), Tag::Int(1));
            let mut nbt = Compound::new();
            nbt.insert("Item".into(), Tag::Compound(stack));
            nbt.insert("PickupDelay".into(), Tag::Short(20));
            nbt.insert("Motion".into(), motion_tag(v));
            if host.summon(&dim, "minecraft:item", at, nbt).is_some() {
                thrown += 1;
            }
        }
    }
    let _ = host.play_sound("minecraft:entity.skeleton.death", pos, HOSTILE, 2.0, 0.6);
    let _ = host.play_sound("minecraft:block.bone_block.break", pos, HOSTILE, 2.0, 0.7);
    if by_player {
        // `Enemy.XP_REWARD_HUGE` (50): the zombie class it copies gives 5.
        let mut orb = Compound::new();
        orb.insert("Value".into(), Tag::Short(45));
        let _ = host.summon(&dim, "minecraft:experience_orb", [pos[0], pos[1] + 0.5, pos[2]], orb);
    }
    trace(format_args!("skeleton {entity} shatters: {hurt} hurt, {thrown} parts thrown (by player {by_player})"));
    Flow::Continue
}

fn motion_tag(v: [f64; 3]) -> Tag {
    Tag::List(v.iter().map(|c| Tag::Double(c.to_bits())).collect())
}

/// One tick of a Mutant Skeleton on the server, after the game ticked it.
pub fn entity_tick_post(host: &mut dyn Host, dim: &str, id: u32, now: u64) {
    let Some(brain) = get(id) else { return };
    let key = (dim.to_owned(), id);
    let first = runtimes().lock().map(|m| !m.contains_key(&key)).unwrap_or(false);
    if first {
        if let Ok(mut map) = runtimes().lock() {
            map.insert(
                key.clone(),
                Runtime {
                    last_health: brain.health,
                    ambient: 0,
                    arrows: Vec::new(),
                    rng: Rng::new(u64::from(id) ^ now ^ 0x5CE1),
                    last_seen: now,
                },
            );
        }
        if let Ok(mut s) = ids().lock() {
            s.insert(id as i32);
        }
        if let Some(e) = host.entity(id) {
            let mut nbt = Compound::new();
            nbt.insert("IsBaby".into(), Tag::Byte(0));
            nbt.insert("CanPickUpLoot".into(), Tag::Byte(0));
            nbt.insert("CanBreakDoors".into(), Tag::Byte(0));
            // The zombie's groans are not a skeleton's: its sounds are played here.
            nbt.insert("Silent".into(), Tag::Byte(1));
            let _ = host.entity_merge_nbt(e, nbt);
        }
    }
    let players = host.server_players();
    let Ok(mut map) = runtimes().lock() else { return };
    let Some(rt) = map.get_mut(&key) else { return };
    rt.last_seen = now;
    let pos = brain.pos;

    // `getHurtSound`, `getAmbientSound` (`Mob.baseTick`).
    if brain.health < rt.last_health - 0.01 && brain.health > 0.0 {
        let _ = host.play_sound("minecraft:entity.skeleton.hurt", pos, HOSTILE, 1.5, 0.5 + rt.rng.float() * 0.2);
        rt.ambient = -80;
    }
    rt.last_health = brain.health;
    rt.ambient += 1;
    if rt.ambient > 0 && (rt.rng.int(1000) as i32) < rt.ambient {
        rt.ambient = -80;
        let _ = host.play_sound("minecraft:entity.skeleton.ambient", pos, HOSTILE, 1.5, 0.5 + rt.rng.float() * 0.2);
    }
    // `aiStep`: two health back every five seconds in the dark.
    if brain.age % 100 == 0 && logic::is_night(host.day_time()) && brain.health > 0.0 && brain.health < MAX_HEALTH {
        let _ = host.set_mob_health(dim, id, (brain.health + 2.0).min(MAX_HEALTH));
    }
    // `MutantArrow.tick`: an arrow that hit nothing is gone after 200 ticks.
    if !rt.arrows.is_empty() {
        let mut keep = Vec::with_capacity(rt.arrows.len());
        for (arrow, born) in rt.arrows.drain(..) {
            if now.saturating_sub(born) > ARROW_LIFE {
                let _ = host.remove_entity(dim, arrow);
            } else {
                keep.push((arrow, born));
            }
        }
        rt.arrows = keep;
    }

    let mut b = brain;
    if b.anim == Anim::None {
        if let Some(anim) = b.request.take() {
            b.anim = anim;
            b.anim_tick = 0;
            broadcast(host, &players, id, pos, anim, 0);
            trace(format_args!("skeleton {id} starts {anim:?}"));
        }
    }
    b.request = None;
    if b.anim != Anim::None {
        let target = b.target.and_then(|t| resolve(host, dim, &t, &players));
        let tick = b.anim_tick;
        match b.anim {
            Anim::Melee => tick_melee(host, dim, id, &b, rt, &players, tick),
            Anim::ConstrictRibs => tick_constrict(host, dim, id, &b, rt, target, tick),
            Anim::Shoot => tick_shoot(host, dim, id, &b, rt, target, tick, now),
            Anim::MultiShot => tick_multi_shot(host, dim, id, &b, rt, target, tick, now),
            Anim::None => {}
        }
        b.anim_tick += 1;
        if b.anim_tick >= b.anim.duration() {
            b.anim = Anim::None;
            b.anim_tick = 0;
            broadcast(host, &players, id, pos, Anim::None, 0);
        } else if b.anim_tick % 10 == 0 {
            // For a player who came into view meanwhile.
            broadcast(host, &players, id, pos, b.anim, b.anim_tick);
        }
    }
    drop(map);
    with(id, |w| {
        w.anim = b.anim;
        w.anim_tick = b.anim_tick;
        w.request = None;
    });
}

/// `MeleeGoal.tick`: at tick 3 everything living within 3 blocks, in front of
/// it, is hit and knocked away.
fn tick_melee(host: &mut dyn Host, dim: &str, id: u32, b: &Brain, rt: &mut Runtime, players: &[NativeServerPlayer], tick: u32) {
    if tick != 3 {
        return;
    }
    let pos = b.pos;
    let mut hit = 0;
    for p in players.iter().filter(|p| p.dimension == dim && p.entity_id > 0 && matches!(p.game_mode, 0 | 2)) {
        let Some(v) = melee_push(&mut rt.rng, pos, b.head_yaw, p.position, 0.0) else { continue };
        let damage = ATTACK_DAMAGE + rt.rng.int(2) as f32;
        let _ = host.hurt_entity(dim, p.entity_id as u32, damage, "minecraft:mob_attack", Some(id));
        let msg = PushMessage([v[0] as f32, v[1] as f32, v[2] as f32]);
        let _ = host.net_send_to_player(p.uuid, crate::net::PUSH, &msg.encode());
        hit += 1;
    }
    let half = WIDTH / 2.0 + 4.0;
    let min = [pos[0] - half, pos[1] - 4.0, pos[2] - half];
    let max = [pos[0] + half, pos[1] + HEIGHT + 4.0, pos[2] + half];
    for other in host.entities_in(dim, min, max) {
        if other == id || players.iter().any(|p| p.entity_id == other as i32) {
            continue;
        }
        if ids().lock().map(|s| s.contains(&(other as i32))).unwrap_or(false) {
            continue;
        }
        let Some(e) = host.entity(other) else { continue };
        if host.living(e).is_none() {
            continue;
        }
        let Some(at) = host.entity_position(e) else { continue };
        let vy = host.entity_velocity(e).map(|v| v[1]).unwrap_or(0.0);
        let Some(v) = melee_push(&mut rt.rng, pos, b.head_yaw, at, vy) else { continue };
        let damage = ATTACK_DAMAGE + rt.rng.int(2) as f32;
        let _ = host.hurt_entity(dim, other, damage, "minecraft:mob_attack", Some(id));
        let _ = host.entity_set_velocity(e, v);
        hit += 1;
    }
    let pitch = 1.0 / (rt.rng.float() * 0.4 + 1.2);
    let _ = host.play_sound("minecraft:entity.player.attack.knockback", pos, HOSTILE, 1.0, pitch);
    trace(format_args!("skeleton {id} swings: {hit} hit"));
}

/// `ConstrictRibsGoal.tick`: at tick 6 the ribs close on the target (attack
/// damage plus 6) and fling it away.
fn tick_constrict(host: &mut dyn Host, dim: &str, id: u32, b: &Brain, rt: &mut Runtime, target: Option<Resolved>, tick: u32) {
    if tick != 6 {
        return;
    }
    let Some(t) = target else { return };
    let hurt = host.hurt_entity(dim, t.entity, ATTACK_DAMAGE + 6.0, "minecraft:mob_attack", Some(id));
    let v = constrict_push(&mut rt.rng);
    match t.player {
        Some(uuid) => {
            let msg = PushMessage([v[0] as f32, v[1] as f32, v[2] as f32]);
            let _ = host.net_send_to_player(uuid, crate::net::PUSH, &msg.encode());
        }
        None => {
            if let Some(e) = host.entity(t.entity) {
                let _ = host.entity_set_velocity(e, v);
            }
        }
    }
    let pitch = 0.8 + rt.rng.float() * 0.4;
    let _ = host.play_sound("minecraft:entity.generic.explode", b.pos, HOSTILE, 0.5, pitch);
    trace(format_args!("skeleton {id} constricts {} : {hurt}", t.entity));
}

/// `ShootGoal.tick`: one arrow at tick 26, sometimes poisoned or slowing.
#[allow(clippy::too_many_arguments)]
fn tick_shoot(host: &mut dyn Host, dim: &str, id: u32, b: &Brain, rt: &mut Runtime, target: Option<Resolved>, tick: u32, now: u64) {
    let pos = b.pos;
    match tick {
        5 => {
            let _ = host.play_sound("minecraft:item.crossbow.quick_charge_2", pos, HOSTILE, 1.0, 1.0);
        }
        20 => {
            let pitch = 1.0 / (rt.rng.float() * 0.5 + 1.0) + 0.2;
            let _ = host.play_sound("minecraft:item.crossbow.loading_end", pos, HOSTILE, 1.0, pitch);
        }
        26 => {
            let Some(t) = target else { return };
            // Just hit, it aims worse; a target out of sight, worse still.
            let randomization = if b.hurt_time > 0 {
                b.hurt_time as f32 / 2.0
            } else if !b.sees {
                0.5 + rt.rng.float()
            } else {
                0.0
            };
            // Upstream rolls poison, hunger and slowness one time in four
            // each; an arrow here carries one potion at most.
            let poison = rt.rng.int(4) == 0;
            let _hunger = rt.rng.int(4) == 0;
            let slowness = rt.rng.int(4) == 0;
            let potion = if poison {
                Some("minecraft:poison")
            } else if slowness {
                Some("minecraft:slowness")
            } else {
                None
            };
            let arrow = fire_arrow(host, dim, id, pos, t.aim, 2.4, randomization, potion, &mut rt.rng);
            if let Some(a) = arrow {
                rt.arrows.push((a, now));
            }
            let pitch = 1.0 / (rt.rng.float() * 0.4 + 1.2) + 0.25;
            let _ = host.play_sound("minecraft:item.crossbow.shoot", pos, HOSTILE, 1.0, pitch);
            trace(format_args!("skeleton {id} shoots at {:?} ({potion:?}): {arrow:?}", t.aim));
        }
        _ => {}
    }
}

/// `MultiShotGoal.tick`: a leap at tick 10, then three volleys of six arrows
/// from the air (upstream readies six on each of ticks 24 to 27 and lets the
/// ones before go: 25, 26 and 27 here).
#[allow(clippy::too_many_arguments)]
fn tick_multi_shot(host: &mut dyn Host, dim: &str, id: u32, b: &Brain, rt: &mut Runtime, target: Option<Resolved>, tick: u32, now: u64) {
    let Some(t) = target else { return };
    let pos = b.pos;
    match tick {
        10 => {
            if let Some(e) = host.entity(id) {
                let v = multi_shot_leap(&mut rt.rng, pos, t.pos);
                let _ = host.entity_set_velocity(e, v);
                trace(format_args!("skeleton {id} leaps {v:?}"));
            }
        }
        15 => {
            let _ = host.play_sound("minecraft:item.crossbow.quick_charge_3", pos, HOSTILE, 1.0, 1.0);
        }
        20 => {
            let pitch = 1.0 / (rt.rng.float() * 0.5 + 1.0) + 0.2;
            let _ = host.play_sound("minecraft:item.crossbow.loading_end", pos, HOSTILE, 1.0, pitch);
        }
        24..=27 => {
            if tick > 24 {
                // Where it is now, mid-leap, not where the goals last saw it.
                let from = host.entity(id).and_then(|e| host.entity_position(e)).unwrap_or(pos);
                let mut shot = 0;
                for _ in 0..6 {
                    let velocity = 2.0 - rt.rng.float() * 0.1;
                    if let Some(a) = fire_arrow(host, dim, id, from, t.aim, velocity, 3.0, None, &mut rt.rng) {
                        rt.arrows.push((a, now));
                        shot += 1;
                    }
                }
                trace(format_args!("skeleton {id} volley: {shot} arrows"));
            }
            let pitch = 1.0 / (rt.rng.float() * 0.4 + 1.2) + 0.25;
            let _ = host.play_sound("minecraft:item.crossbow.shoot", pos, HOSTILE, 1.0, pitch);
        }
        _ => {}
    }
}

/// `new MutantArrow(level, shooter)` and `shoot`: a vanilla arrow with no
/// gravity, critical, owned by the skeleton, from just under its eyes.
#[allow(clippy::too_many_arguments)]
fn fire_arrow(
    host: &mut dyn Host,
    dim: &str,
    id: u32,
    feet: [f64; 3],
    aim: [f64; 3],
    velocity: f32,
    randomization: f32,
    potion: Option<&str>,
    rng: &mut Rng,
) -> Option<u32> {
    let from = [feet[0], feet[1] + EYE_HEIGHT - 0.1, feet[2]];
    let motion = arrow_motion(rng, from, aim, velocity, randomization);
    let mut nbt = Compound::new();
    nbt.insert("Motion".into(), motion_tag(motion));
    nbt.insert("NoGravity".into(), Tag::Byte(1));
    nbt.insert("crit".into(), Tag::Byte(1));
    nbt.insert("pickup".into(), Tag::Byte(0));
    nbt.insert("damage".into(), Tag::Double(2.0f64.to_bits()));
    nbt.insert("HasBeenShot".into(), Tag::Byte(1));
    // Stuck in a block, it is gone within a second (`onHit` discards it).
    nbt.insert("life".into(), Tag::Short(1180));
    if let Some(uuid) = host.entity(id).and_then(|e| host.entity_uuid(e)) {
        nbt.insert("Owner".into(), Tag::IntArray(uuid_ints(uuid).to_vec()));
    }
    if let Some(potion) = potion {
        let mut contents = Compound::new();
        contents.insert("potion".into(), Tag::String(potion.into()));
        let mut components = Compound::new();
        components.insert("minecraft:potion_contents".into(), Tag::Compound(contents));
        let mut item = Compound::new();
        item.insert("id".into(), Tag::String("minecraft:tipped_arrow".into()));
        item.insert("count".into(), Tag::Int(1));
        item.insert("components".into(), Tag::Compound(components));
        nbt.insert("item".into(), Tag::Compound(item));
    }
    host.summon(dim, "minecraft:arrow", from, nbt)
}

#[derive(Clone, Copy, Debug)]
struct Resolved {
    entity: u32,
    player: Option<u128>,
    pos: [f64; 3],
    /// `target.getY(0.8)`: where the arrows go.
    aim: [f64; 3],
}

fn resolve(host: &mut dyn Host, dim: &str, t: &TargetSnap, players: &[NativeServerPlayer]) -> Option<Resolved> {
    if let Some(mob) = t.mob {
        let e = host.entity(mob)?;
        let pos = host.entity_position(e)?;
        let height = host.entity_bounding_box(e).map(|b| b[1][1] - b[0][1]).unwrap_or(1.8);
        return Some(Resolved { entity: mob, player: None, pos, aim: [pos[0], pos[1] + height * 0.8, pos[2]] });
    }
    players
        .iter()
        .filter(|p| p.dimension == dim && p.entity_id > 0 && p.health > 0.0 && matches!(p.game_mode, 0 | 2))
        .map(|p| (logic::dist_sqr(p.position, t.pos), p))
        .filter(|(d, _)| *d < 16.0)
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, p)| Resolved {
            entity: p.entity_id as u32,
            player: Some(p.uuid),
            pos: p.position,
            aim: [p.position[0], p.position[1] + 1.8 * 0.8, p.position[2]],
        })
}

fn broadcast(host: &mut dyn Host, players: &[NativeServerPlayer], id: u32, pos: [f64; 3], anim: Anim, elapsed: u32) {
    let msg = AnimMessage { entity: id, anim: anim.id(), elapsed: elapsed.min(u32::from(u16::MAX)) as u16, flags: 0 };
    let bytes = msg.encode();
    for p in players {
        if logic::dist_sqr(p.position, pos) <= VIEW * VIEW {
            let _ = host.net_send_to_player(p.uuid, CHANNEL, &bytes);
        }
    }
}

/// Forget skeletons that stopped ticking (dead, unloaded or removed).
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
    if let Ok(mut s) = ids().lock() {
        for id in &gone {
            s.remove(&(*id as i32));
        }
    }
}

// --- client --------------------------------------------------------------------------------

/// `MutantSkeletonModel.Spine.createSpineLayer(root, index)` for the three
/// spine pieces of the body (`index` 0 to 2).
fn spine_layer(mesh: &mut MeshDefinition, parent: PartId, index: usize) -> PartId {
    let spine_scale = if index == 1 { 0.98 } else { 1.0 };
    let y = if index == 0 { -7.0 } else { -5.0 };
    let n = index + 1;
    let rib = |mirror: bool, x: f32, grow: f32| {
        let b = CubeListBuilder::create().tex_offs(32, 12);
        let b = if mirror { b.mirror() } else { b };
        b.add_box_with([x, -2.0, -2.0], [6.0, 2.0, 2.0], CubeDeformation::new(grow))
    };
    let middle = mesh.add_or_replace_child(
        parent,
        &format!("middle{n}"),
        CubeListBuilder::create().tex_offs(50, 0).add_box_with([-2.5, -4.0, -2.0], [5.0, 4.0, 4.0], CubeDeformation::new(0.5)),
        PartPose::offset_and_rotation(0.0, y, 0.0, PI / 18.0, 0.0, 0.0),
    );
    let side11 = mesh.add_or_replace_child(
        middle,
        &format!("side11{n}"),
        rib(false, -6.0, 0.25),
        PartPose::offset_and_rotation(-3.0, -1.0, 1.75, 0.0, -PI / 4.5 * spine_scale, 0.0),
    );
    let side12 = mesh.add_or_replace_child(
        side11,
        &format!("side12{n}"),
        rib(true, -6.0, 0.2),
        PartPose::offset_and_rotation(-6.5, 0.0, 0.0, 0.0, -PI / 3.0 * spine_scale, 0.0),
    );
    mesh.add_or_replace_child(
        side12,
        &format!("side13{n}"),
        rib(false, -6.0, 0.15),
        PartPose::offset_and_rotation(-6.4, 0.0, 0.0, 0.0, -PI / 3.5 * spine_scale, 0.0),
    );
    let side21 = mesh.add_or_replace_child(
        middle,
        &format!("side21{n}"),
        rib(true, 0.0, 0.25),
        PartPose::offset_and_rotation(3.0, -1.0, 1.75, 0.0, PI / 4.5 * spine_scale, 0.0),
    );
    let side22 = mesh.add_or_replace_child(
        side21,
        &format!("side22{n}"),
        rib(false, 0.0, 0.2),
        PartPose::offset_and_rotation(6.5, 0.0, 0.0, 0.0, PI / 3.0 * spine_scale, 0.0),
    );
    mesh.add_or_replace_child(
        side22,
        &format!("side23{n}"),
        rib(true, 0.0, 0.15),
        PartPose::offset_and_rotation(6.4, 0.0, 0.0, 0.0, PI / 3.5 * spine_scale, 0.0),
    );
    middle
}

/// `MutantSkeletonModel.createBodyLayer()`, 128x128.
pub fn layer() -> LayerDefinition {
    let mut mesh = MeshDefinition::new();
    let root = mesh.root();
    let empty = || CubeListBuilder::create();
    let pose = PartPose::offset_and_rotation;
    let limb = |u: i32, v: i32, h: f32| CubeListBuilder::create().tex_offs(u, v).add_box([-2.0, 0.0, -2.0], [4.0, h, 4.0]);

    let base = mesh.add_or_replace_child(root, "base", empty(), pose(0.0, 3.0, 0.0, 0.0, 0.0, 0.0));
    let pelvis = mesh.add_or_replace_child(
        base,
        "pelvis",
        CubeListBuilder::create().tex_offs(0, 16).add_box([-4.0, -6.0, -3.0], [8.0, 6.0, 6.0]),
        pose(0.0, 0.0, 0.0, -PI / 10.0, 0.0, 0.0),
    );
    let mut middle = mesh.add_or_replace_child(
        pelvis,
        "waist",
        CubeListBuilder::create().tex_offs(32, 0).add_box([-2.5, -8.0, -2.0], [5.0, 8.0, 4.0]),
        pose(0.0, -5.0, 0.0, PI / 14.0, 0.0, 0.0),
    );
    for i in 0..3 {
        middle = spine_layer(&mut mesh, middle, i);
    }
    let neck = mesh.add_or_replace_child(
        middle,
        "neck",
        CubeListBuilder::create().tex_offs(64, 0).add_box([-1.5, -4.0, -1.5], [3.0, 4.0, 3.0]),
        pose(0.0, -4.0, 0.0, -PI / 24.0, 0.0, 0.0),
    );
    let head = mesh.add_or_replace_child(neck, "head", empty(), pose(0.0, -4.0, -1.0, -PI / 24.0, 0.0, 0.0));
    let inner_head = mesh.add_or_replace_child(
        head,
        "inner_head",
        CubeListBuilder::create().tex_offs(0, 0).add_box_with([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], CubeDeformation::new(0.4)),
        pose(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
    );
    mesh.add_or_replace_child(
        inner_head,
        "jaw",
        CubeListBuilder::create().tex_offs(72, 0).add_box_with([-4.0, -3.0, -8.0], [8.0, 3.0, 8.0], CubeDeformation::new(0.7)),
        pose(0.0, -0.2, 3.5, PI / 32.0, 0.0, 0.0),
    );

    for (side, sign, mirror) in [("right", -1.0f32, false), ("left", 1.0f32, true)] {
        let shoulder_box = CubeListBuilder::create().tex_offs(28, 16);
        let shoulder_box = if mirror { shoulder_box.mirror() } else { shoulder_box };
        let shoulder = mesh.add_or_replace_child(
            middle,
            &format!("{side}_shoulder"),
            shoulder_box.add_box([-4.0, -3.0, -3.0], [8.0, 3.0, 6.0]),
            pose(7.0 * sign, -3.0, -1.0, -PI / 4.0, 0.0, 0.0),
        );
        let arm = mesh.add_or_replace_child(shoulder, &format!("{side}_arm"), empty(), pose(sign, -1.0, 0.0, 0.0, 0.0, 0.0));
        let arm_inner = mesh.add_or_replace_child(
            arm,
            &format!("{side}_arm_inner"),
            limb(0, 28, 12.0),
            pose(0.0, 0.0, 0.0, PI / 6.0, 0.0, -sign * PI / 10.0),
        );
        let arm_lower =
            mesh.add_or_replace_child(arm_inner, &format!("{side}_arm_lower"), empty(), pose(0.0, 11.0, 0.0, 0.0, 0.0, 0.0));
        mesh.add_or_replace_child(
            arm_lower,
            &format!("{side}_arm_lower_inner"),
            CubeListBuilder::create().tex_offs(16, 28).add_box_with([-2.0, 0.0, -2.0], [4.0, 14.0, 4.0], CubeDeformation::new(-0.01)),
            pose(0.0, 0.0, 0.0, -PI / 6.0, 0.0, 0.0),
        );

        let leg = mesh.add_or_replace_child(
            pelvis,
            &format!("{side}_leg"),
            empty(),
            pose(2.5 * sign, -2.5, 0.0, PI / 60.0, 0.0, -sign * PI / 16.0),
        );
        let leg_inner =
            mesh.add_or_replace_child(leg, &format!("{side}_leg_inner"), limb(0, 28, 12.0), pose(0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        let leg_lower = mesh.add_or_replace_child(
            leg_inner,
            &format!("{side}_leg_lower"),
            empty(),
            pose(0.0, 12.0, 0.0, 0.0, 0.0, sign * PI / 24.0),
        );
        mesh.add_or_replace_child(
            leg_lower,
            &format!("{side}_leg_lower_inner"),
            limb(32, 28, 12.0),
            pose(0.0, 0.0, 0.0, PI / 10.0, 0.0, 0.0),
        );
    }
    LayerDefinition::create(mesh, 128, 128)
}

/// The right or the left limb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Right,
    Left,
}

impl Side {
    fn name(self) -> &'static str {
        match self {
            Side::Right => "right",
            Side::Left => "left",
        }
    }
}

/// The parts `setupAnim` moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum P {
    Base,
    Waist,
    /// `spine[i].middle`.
    Mid(usize),
    /// `spine[i].side1[j]` and `side2[j]`.
    Side1(usize, usize),
    Side2(usize, usize),
    Neck,
    Head,
    InnerHead,
    Jaw,
    Arm(Side),
    ArmInner(Side),
    ArmLower(Side),
    Leg(Side),
    LegLower(Side),
    LegLowerInner(Side),
}

impl P {
    /// The part's path from the model's root.
    pub fn path(self) -> Vec<String> {
        let mut path: Vec<String> = vec!["base".into(), "pelvis".into()];
        let spine = |path: &mut Vec<String>, upto: usize| {
            path.push("waist".into());
            for i in 0..=upto {
                path.push(format!("middle{}", i + 1));
            }
        };
        let ribs = |path: &mut Vec<String>, which: u8, i: usize, j: usize| {
            for k in 0..=j {
                path.push(format!("side{which}{}{}", k + 1, i + 1));
            }
        };
        let arm = |path: &mut Vec<String>, s: Side, depth: usize| {
            let n = s.name();
            let names = [format!("{n}_shoulder"), format!("{n}_arm"), format!("{n}_arm_inner"), format!("{n}_arm_lower")];
            path.extend(names.into_iter().take(depth));
        };
        let leg = |path: &mut Vec<String>, s: Side, depth: usize| {
            let n = s.name();
            let names =
                [format!("{n}_leg"), format!("{n}_leg_inner"), format!("{n}_leg_lower"), format!("{n}_leg_lower_inner")];
            path.extend(names.into_iter().take(depth));
        };
        match self {
            P::Base => {
                path.pop();
            }
            P::Waist => path.push("waist".into()),
            P::Mid(i) => spine(&mut path, i),
            P::Side1(i, j) => {
                spine(&mut path, i);
                ribs(&mut path, 1, i, j);
            }
            P::Side2(i, j) => {
                spine(&mut path, i);
                ribs(&mut path, 2, i, j);
            }
            P::Neck | P::Head | P::InnerHead | P::Jaw => {
                spine(&mut path, 2);
                let names = ["neck", "head", "inner_head", "jaw"];
                let depth = match self {
                    P::Neck => 1,
                    P::Head => 2,
                    P::InnerHead => 3,
                    _ => 4,
                };
                path.extend(names.iter().take(depth).map(|s| (*s).to_owned()));
            }
            P::Arm(s) => {
                spine(&mut path, 2);
                arm(&mut path, s, 2);
            }
            P::ArmInner(s) => {
                spine(&mut path, 2);
                arm(&mut path, s, 3);
            }
            P::ArmLower(s) => {
                spine(&mut path, 2);
                arm(&mut path, s, 4);
            }
            P::Leg(s) => leg(&mut path, s, 1),
            P::LegLower(s) => leg(&mut path, s, 3),
            P::LegLowerInner(s) => leg(&mut path, s, 4),
        }
        path
    }
}

/// What `setupAnim` adds to a part's initial pose, and its scale if set.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Delta {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub xr: f32,
    pub yr: f32,
    pub zr: f32,
    pub scale: Option<f32>,
}

/// The whole pose: each moved part with what is added to it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose(pub Vec<(P, Delta)>);

impl Pose {
    pub fn at(&mut self, p: P) -> &mut Delta {
        let i = match self.0.iter().position(|(q, _)| *q == p) {
            Some(i) => i,
            None => {
                self.0.push((p, Delta::default()));
                self.0.len() - 1
            }
        };
        &mut self.0[i].1
    }

    pub fn get(&self, p: P) -> Delta {
        self.0.iter().find(|(q, _)| *q == p).map(|(_, d)| *d).unwrap_or_default()
    }

    /// `Animator`'s `Transform.rotate`/`offset` with a weight.
    fn add(&mut self, p: P, rot: [f32; 3], off: [f32; 3], w: f32) {
        let d = self.at(p);
        d.xr += rot[0] * w;
        d.yr += rot[1] * w;
        d.zr += rot[2] * w;
        d.x += off[0] * w;
        d.y += off[1] * w;
        d.z += off[2] * w;
    }
}

const R: Side = Side::Right;
const L: Side = Side::Left;

/// `MutantSkeletonModel.setupAnim`, for a right-handed skeleton. `anim_time`
/// is the animation's tick with the partial tick; yaw and pitch in degrees.
pub fn setup_anim(walk_position: f32, walk_speed: f32, age: f32, head_yaw: f32, pitch: f32, anim: Anim, anim_time: f32) -> Pose {
    let mut p = Pose::default();
    let speed = walk_speed;
    let mut walk = (walk_position * 0.5).sin();
    let mut lower_leg_walk = (walk_position * 0.5 - 1.1).sin();
    let breathe = (age * 0.1).sin();
    let mut face_yaw = head_yaw * PI / 180.0;
    let mut face_pitch = pitch * PI / 180.0;
    match anim {
        Anim::Melee => {
            animate_melee(&mut p, anim_time);
            let scale = 1.0 - (anim_time / 4.0).clamp(0.0, 1.0);
            walk *= scale;
            lower_leg_walk *= scale;
        }
        Anim::Shoot => {
            animate_shoot(&mut p, anim_time, face_pitch, face_yaw);
            let scale = 1.0 - (anim_time / 4.0).clamp(0.0, 1.0);
            walk *= scale;
            lower_leg_walk *= scale;
            face_pitch *= scale;
            face_yaw *= scale;
        }
        Anim::MultiShot => {
            animate_multi_shoot(&mut p, anim_time, face_pitch, face_yaw);
            let scale = 1.0 - (anim_time / 4.0).clamp(0.0, 1.0);
            walk *= scale;
            lower_leg_walk *= scale;
            face_pitch *= scale;
            face_yaw *= scale;
        }
        Anim::ConstrictRibs => {
            animate_constrict(&mut p, anim_time);
            let scale = 1.0 - (anim_time / 6.0).clamp(0.0, 1.0);
            face_pitch *= scale;
            face_yaw *= scale;
        }
        Anim::None => {}
    }

    p.at(P::Base).y -= (-0.5 + walk.abs()) * speed;
    p.at(P::Mid(0)).yr -= walk * 0.06 * speed;
    p.at(P::Arm(R)).xr -= walk * 0.9 * speed;
    p.at(P::Arm(L)).xr += walk * 0.9 * speed;
    p.at(P::Leg(R)).xr += (0.2 + walk) * speed;
    p.at(P::Leg(L)).xr -= (-0.2 + walk) * speed;
    p.at(P::LegLowerInner(R)).xr += (0.6 + lower_leg_walk) * 0.6 * speed;
    p.at(P::LegLowerInner(L)).xr -= (-0.6 + lower_leg_walk) * 0.6 * speed;
    for i in 0..3 {
        // `Spine.animate`.
        p.at(P::Side1(i, 1)).yr += breathe * 0.02;
        p.at(P::Side2(i, 1)).yr -= breathe * 0.02;
    }
    p.at(P::Head).xr -= breathe * 0.02;
    p.at(P::Jaw).xr += breathe * 0.04 + 0.04;
    p.at(P::Arm(R)).zr += breathe * 0.025;
    p.at(P::Arm(L)).zr -= breathe * 0.025;
    p.at(P::InnerHead).xr += face_pitch;
    p.at(P::InnerHead).yr += face_yaw;
    p
}

fn spine_yaw(p: &mut Pose, amount: f32) {
    for i in 0..3 {
        p.at(P::Mid(i)).yr += amount;
    }
}

/// `animateMelee`: the right arm swings across.
fn animate_melee(p: &mut Pose, t: f32) {
    let (melee, off) = (P::Arm(R), P::Arm(L));
    if t < 3.0 {
        let a = (t / 3.0 * PI / 2.0).sin();
        spine_yaw(p, a * PI / 16.0);
        p.at(melee).yr += a * PI / 10.0;
        p.at(melee).zr += a * PI / 4.0;
        p.at(off).zr += a * -PI / 16.0;
    } else if t < 5.0 {
        let a = ((t - 3.0) / 2.0 * PI / 2.0).cos();
        spine_yaw(p, a * 3.0 * PI / 16.0 - PI / 8.0);
        p.at(melee).yr += a * 2.730_769_2 - 2.416_61;
        p.at(melee).zr += a * 3.0 * PI / 8.0 - PI / 8.0;
        p.at(off).zr += -PI / 16.0;
    } else if t < 8.0 {
        spine_yaw(p, -PI / 8.0);
        p.at(melee).yr += -2.416_61;
        p.at(melee).zr += -PI / 8.0;
        p.at(off).zr += -PI / 16.0;
    } else if t < 14.0 {
        let a = ((t - 8.0) / 6.0 * PI / 2.0).cos();
        spine_yaw(p, a * -PI / 8.0);
        p.at(melee).yr += a * -PI / 1.3;
        p.at(melee).zr += a * -PI / 8.0;
        p.at(off).zr += a * -PI / 16.0;
    }
}

const DRAWING: Side = R;
const HOLDING: Side = L;

/// The first phase of both shots: the arms come up (`drawAmount` 0 to 1).
fn raise_bow(p: &mut Pose, a: f32) {
    p.at(P::ArmInner(DRAWING)).xr += -a * PI / 4.0;
    p.at(P::Arm(DRAWING)).yr += -a * PI / 2.0;
    p.at(P::Arm(DRAWING)).zr += a * PI / 16.0;
    p.at(P::ArmLower(DRAWING)).xr += a * PI / 7.0;
    p.at(P::ArmInner(HOLDING)).xr += -a * PI / 4.0;
    p.at(P::Arm(HOLDING)).yr += a * PI / 2.0;
    p.at(P::Arm(HOLDING)).zr += -a * PI / 16.0;
    p.at(P::ArmInner(HOLDING)).zr += -a * PI / 8.0;
    p.at(P::ArmLower(HOLDING)).xr += -a * PI / 6.0;
}

/// The body turned side-on behind the bow, by `turn` (0 to 1).
fn turn_to_aim(p: &mut Pose, turn: f32, face_pitch: f32, face_yaw: f32) {
    p.at(P::InnerHead).yr += turn * PI / 4.0;
    for i in 0..3 {
        let m = p.at(P::Mid(i));
        m.yr += -turn * PI / 12.0;
        m.xr += turn * face_pitch / 3.0;
        m.yr += turn * face_yaw / 3.0;
    }
}

/// The string drawn back (`drawAmount` 1 to 0 as it is drawn).
fn draw_bow(p: &mut Pose, a: f32) {
    p.at(P::ArmInner(DRAWING)).xr += a * PI / 12.0 - PI / 3.0;
    p.at(P::Arm(DRAWING)).yr += a * -3.0 * PI / 10.0 - PI / 5.0;
    p.at(P::Arm(DRAWING)).zr += a * -0.850_848 + PI / 3.0;
    p.at(P::ArmLower(DRAWING)).xr += PI / 7.0;
    p.at(P::ArmInner(HOLDING)).xr += a * 7.0 * PI / 12.0 - 5.0 * PI / 6.0;
    p.at(P::Arm(HOLDING)).yr += a * 3.0 * PI / 10.0 + PI / 5.0;
    p.at(P::Arm(HOLDING)).zr += a * 0.850_848 - PI / 3.0;
    p.at(P::ArmInner(HOLDING)).zr += -a * PI / 8.0;
    p.at(P::ArmLower(HOLDING)).xr += a * PI / 30.0 - PI / 5.0;
}

/// The bow held drawn (`a` 1), then let down (`a` to 0).
fn hold_bow(p: &mut Pose, a: f32, release: bool) {
    p.at(P::ArmInner(DRAWING)).xr += -a * PI / 3.0;
    p.at(P::Arm(DRAWING)).yr += -a * PI / 5.0;
    p.at(P::Arm(DRAWING)).zr += a * PI / 3.0;
    p.at(P::ArmLower(DRAWING)).xr += a * PI / 7.0;
    // Held: -5 pi / 6; let down: -pi / 1.2 (the same angle upstream).
    p.at(P::ArmInner(HOLDING)).xr += if release { -a * PI / 1.2 } else { -5.0 * PI / 6.0 };
    p.at(P::Arm(HOLDING)).yr += a * PI / 5.0;
    p.at(P::Arm(HOLDING)).zr += -a * PI / 3.0;
    p.at(P::ArmLower(HOLDING)).xr += -a * PI / 5.0;
}

/// `animateShoot`.
fn animate_shoot(p: &mut Pose, t: f32, face_pitch: f32, face_yaw: f32) {
    if t < 5.0 {
        raise_bow(p, (t / 5.0 * PI / 2.0).sin());
    } else if t < 12.0 {
        let progress = (t - 5.0) / 7.0;
        turn_to_aim(p, (progress * PI / 2.0).sin(), face_pitch, face_yaw);
        draw_bow(p, (progress * PI / 2.0).cos());
    } else if t < 26.0 {
        turn_to_aim(p, 1.0, face_pitch, face_yaw);
        hold_bow(p, 1.0, false);
    } else if t < 30.0 {
        let a = ((t - 26.0) / 4.0 * PI / 2.0).cos();
        turn_to_aim(p, a, face_pitch, face_yaw);
        hold_bow(p, a, true);
    }
}

/// The crouch before the leap (`drawAmount` 0 to 1), arms by `arms`.
fn crouch(p: &mut Pose, a: f32) {
    p.at(P::Base).y += a * 3.5;
    p.at(P::Mid(0)).xr += a * PI / 6.0;
    p.at(P::Head).xr += -a * PI / 4.0;
    p.at(P::Arm(R)).xr += a * PI / 6.0;
    p.at(P::Arm(R)).zr += a * PI / 16.0;
    p.at(P::Arm(L)).xr += a * PI / 6.0;
    p.at(P::Arm(L)).zr += -a * PI / 16.0;
    p.at(P::Leg(R)).xr += -a * PI / 8.0;
    p.at(P::Leg(L)).xr += -a * PI / 8.0;
    p.at(P::LegLowerInner(R)).xr += a * PI / 4.0;
    p.at(P::LegLowerInner(L)).xr += a * PI / 4.0;
}

/// Limbs spread in the air, by `turn` (0 to 1).
fn spread(p: &mut Pose, turn: f32) {
    p.at(P::Arm(DRAWING)).zr += -turn * PI / 14.0;
    p.at(P::Arm(HOLDING)).zr += turn * PI / 14.0;
    p.at(P::Leg(R)).zr += -turn * PI / 24.0;
    p.at(P::Leg(L)).zr += turn * PI / 24.0;
    p.at(P::LegLower(R)).zr += turn * PI / 64.0;
    p.at(P::LegLower(L)).zr += -turn * PI / 64.0;
}

/// `animateMultiShoot`.
fn animate_multi_shoot(p: &mut Pose, t: f32, face_pitch: f32, face_yaw: f32) {
    if t < 10.0 {
        crouch(p, (t / 10.0 * PI / 2.0).sin());
    } else if t < 12.0 {
        let progress = (t - 10.0) / 2.0;
        crouch(p, (progress * PI / 2.0).cos());
        spread(p, (progress * PI / 2.0).sin());
    } else if t < 14.0 {
        spread(p, 1.0);
    } else if t < 17.0 {
        let progress = (t - 14.0) / 3.0;
        spread(p, (progress * PI / 2.0).cos());
        raise_bow(p, (progress * PI / 2.0).sin());
    } else if t < 20.0 {
        let progress = (t - 17.0) / 3.0;
        turn_to_aim(p, (progress * PI / 2.0).sin(), face_pitch, face_yaw);
        draw_bow(p, (progress * PI / 2.0).cos());
    } else if t < 24.0 {
        turn_to_aim(p, 1.0, face_pitch, face_yaw);
        hold_bow(p, 1.0, false);
    } else if t < 28.0 {
        let a = ((t - 24.0) / 4.0 * PI / 2.0).cos();
        turn_to_aim(p, a, face_pitch, face_yaw);
        hold_bow(p, a, true);
    }
}

/// One `Animator` phase's transforms: part, rotation, offset.
type Phase = Vec<(P, [f32; 3], [f32; 3])>;

/// The weights of `animateConstrict`'s two phases at tick `t`:
/// `startPhase(5)`, `setStationaryPhase(2)`, `startPhase(1)`,
/// `setStationaryPhase(4)`, `resetPhase(8)`.
pub fn constrict_weights(t: f32) -> (f32, f32) {
    let ease = |from: f32, len: f32| ((t - from) / len * PI / 2.0).sin();
    if t < 0.0 {
        (0.0, 0.0)
    } else if t < 5.0 {
        (ease(0.0, 5.0), 0.0)
    } else if t < 7.0 {
        (1.0, 0.0)
    } else if t < 8.0 {
        let inc = ease(7.0, 1.0);
        (1.0 - inc, inc)
    } else if t < 12.0 {
        (0.0, 1.0)
    } else if t < 20.0 {
        (0.0, 1.0 - ease(12.0, 8.0))
    } else {
        (0.0, 0.0)
    }
}

/// The ribs' swelling in `animateConstrict` (`Animator.setScale`).
pub fn rib_scale(t: f32) -> f32 {
    if t < 5.0 {
        1.0 + (t / 5.0 * PI / 2.0).sin() * 0.6
    } else if t < 12.0 {
        1.6
    } else if t < 20.0 {
        1.0 + ((t - 12.0) / 8.0 * PI / 2.0).cos() * 0.6
    } else {
        1.0
    }
}

/// `animateConstrict`: the ribs open wide, then snap shut.
fn animate_constrict(p: &mut Pose, t: f32) {
    let rot = |part: P, x: f32, y: f32, z: f32| (part, [x, y, z], [0.0, 0.0, 0.0]);
    let legs = |phase: &mut Phase| {
        phase.push((P::Base, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
        phase.push(rot(P::Leg(R), -PI / 7.0, 0.0, 0.0));
        phase.push(rot(P::Leg(L), -PI / 7.0, 0.0, 0.0));
        phase.push(rot(P::LegLowerInner(R), PI / 6.0, 0.0, 0.0));
        phase.push(rot(P::LegLowerInner(L), PI / 6.0, 0.0, 0.0));
    };

    let mut open: Phase = vec![rot(P::Waist, PI / 24.0, 0.0, 0.0)];
    for i in 0..3 {
        let rib_x = if i == 0 { PI / 8.0 } else if i == 2 { -PI / 8.0 } else { 0.0 };
        let rib_y = if i == 1 { PI / 8.0 } else { PI / 10.0 };
        open.push(rot(P::Side1(i, 0), rib_x, rib_y, 0.0));
        open.push(rot(P::Side1(i, 1), 0.0, PI / 20.0, 0.0));
        open.push(rot(P::Side1(i, 2), 0.0, PI / 12.0, 0.0));
        open.push(rot(P::Side2(i, 0), rib_x, -rib_y, 0.0));
        open.push(rot(P::Side2(i, 1), 0.0, -PI / 20.0, 0.0));
        open.push(rot(P::Side2(i, 2), 0.0, -PI / 12.0, 0.0));
    }
    open.push(rot(P::Arm(R), 0.0, 0.0, 2.0 * PI / 7.0));
    open.push(rot(P::Arm(L), 0.0, 0.0, -2.0 * PI / 7.0));
    legs(&mut open);

    let mut shut: Phase = vec![
        rot(P::Neck, PI / 16.0, 0.0, 0.0),
        rot(P::Head, PI / 20.0, 0.0, 0.0),
        rot(P::Waist, PI / 10.0, 0.0, 0.0),
        rot(P::Mid(0), PI / 12.0, 0.0, 0.0),
    ];
    for i in 0..3 {
        let rib_x = if i == 0 { PI / 24.0 } else if i == 2 { -PI / 24.0 } else { 0.0 };
        let rib_y = if i == 1 { -PI / 18.0 } else { -PI / 14.0 };
        shut.push(rot(P::Side1(i, 0), rib_x - 0.08, rib_y, 0.0));
        shut.push(rot(P::Side1(i, 1), 0.0, PI / 20.0, 0.0));
        shut.push(rot(P::Side1(i, 2), 0.0, PI / 12.0, 0.0));
        shut.push(rot(P::Side2(i, 0), rib_x + 0.08, -rib_y, 0.0));
        shut.push(rot(P::Side2(i, 1), 0.0, -PI / 20.0, 0.0));
        shut.push(rot(P::Side2(i, 2), 0.0, -PI / 12.0, 0.0));
    }
    legs(&mut shut);

    let (w_open, w_shut) = constrict_weights(t);
    for (phase, w) in [(&open, w_open), (&shut, w_shut)] {
        if w != 0.0 {
            for (part, r, o) in phase {
                p.add(*part, *r, *o, w);
            }
        }
    }
    let scale = rib_scale(t);
    if t < 20.0 {
        for i in 0..3 {
            p.at(P::Side1(i, 0)).scale = Some(scale);
            p.at(P::Side2(i, 0)).scale = Some(scale);
        }
    }
}

/// What one skeleton is doing, as this client heard it.
#[derive(Clone, Copy, Debug)]
struct Seen {
    anim: Anim,
    start: u32,
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
    let Some(anim) = Anim::from_id(msg.anim) else { return };
    let Ok(mut m) = seen().lock() else { return };
    if anim == Anim::None {
        m.remove(&msg.entity);
        return;
    }
    let start = now.saturating_sub(u32::from(msg.elapsed));
    let e = m.entry(msg.entity).or_insert(Seen { anim, start });
    if e.anim != anim || e.start.abs_diff(start) > 3 {
        *e = Seen { anim, start };
    }
}

pub fn client_tick(now: u32) {
    if now % 100 == 0 {
        if let Ok(mut m) = seen().lock() {
            // An animation the server never ended (the skeleton left view).
            m.retain(|_, s| now.saturating_sub(s.start) < s.anim.duration() + 40);
        }
    }
}

fn part_mut<'a>(root: &'a mut ModelPart, path: &[String]) -> Option<&'a mut ModelPart> {
    let mut at = root;
    for name in path {
        at = at.get_child_mut(name)?;
    }
    Some(at)
}

/// `MutantSkeletonRenderer`: draw it with its own model. `now` is the client
/// tick with the partial tick.
pub fn submit(host: &dyn Host, mob: &NativeMobRender<'_>, draw: &mut BlockEntityDraw, now: f32) -> Flow {
    // `die` sets `deathTime` to 19: it is gone at once, in pieces.
    if mob.death_time > 0 {
        return Flow::Cancel;
    }
    let model = {
        let Ok(mut cache) = baked().lock() else { return Flow::Continue };
        if cache.is_none() {
            *cache = host.bake_layer(LAYER);
        }
        cache.clone()
    };
    let Some(mut model) = model else { return Flow::Continue };
    let state = seen().lock().ok().and_then(|m| m.get(&mob.id).copied());
    let (anim, t) = match state {
        Some(s) => {
            let t = (now - s.start as f32).max(0.0);
            if t < s.anim.duration() as f32 { (s.anim, t) } else { (Anim::None, 0.0) }
        }
        None => (Anim::None, 0.0),
    };
    let pose = setup_anim(mob.walk_position, mob.walk_speed, mob.age, mob.head_yaw, mob.pitch, anim, t);
    model.reset_all_poses();
    for (p, d) in &pose.0 {
        if let Some(part) = part_mut(&mut model, &p.path()) {
            part.x += d.x;
            part.y += d.y;
            part.z += d.z;
            part.x_rot += d.xr;
            part.y_rot += d.yr;
            part.z_rot += d.zr;
            if let Some(s) = d.scale {
                part.x_scale = s;
                part.y_scale = s;
                part.z_scale = s;
            }
        }
    }
    draw.push();
    draw.scale([1.0, -1.0, -1.0]);
    draw.translate([0.0, -1.501, 0.0]);
    let tint = if mob.hurt_time > 0 { 0xFF9999 } else { 0xFFFFFF };
    host.draw_model_part(draw, &model, TEXTURE, tint);
    draw.pop();
    Flow::Cancel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anims_round_trip() {
        for a in [Anim::None, Anim::Melee, Anim::ConstrictRibs, Anim::Shoot, Anim::MultiShot] {
            assert_eq!(Anim::from_id(a.id()), Some(a));
        }
        assert_eq!(Anim::from_id(9), None);
        assert_eq!(Anim::Shoot.duration(), 32);
    }

    #[test]
    fn eighteen_parts() {
        assert_eq!(BODY_PARTS.iter().map(|(_, n)| u32::from(*n)).sum::<u32>(), 18);
    }

    #[test]
    fn shots_need_sight_and_distance() {
        let mut rng = Rng::new(3);
        assert!(!(0..10_000).any(|_| shoot_choice(&mut rng, 100.0, false)));
        assert!(!(0..10_000).any(|_| shoot_choice(&mut rng, 3.0, true)));
        let n = (0..120_000).filter(|_| shoot_choice(&mut rng, 100.0, true)).count();
        assert!((8_000..12_000).contains(&n), "{n}");
        assert!(!(0..10_000).any(|_| multi_shot_choice(&mut rng, 4, true, true)));
        assert!(!(0..10_000).any(|_| multi_shot_choice(&mut rng, 3, false, true)));
        assert!((0..10_000).any(|_| multi_shot_choice(&mut rng, 3, true, true)));
        let swings = (0..40_000).filter(|_| melee_choice(&mut rng) == Anim::Melee).count();
        assert!((29_000..31_000).contains(&swings), "{swings}");
    }

    #[test]
    fn arrows_fly_at_their_speed() {
        let mut rng = Rng::new(7);
        let v = arrow_motion(&mut rng, [0.0, 3.0, 0.0], [10.0, 3.0, 0.0], 2.4, 0.0);
        assert!((v[0] - 2.4).abs() < 1e-6 && v[1].abs() < 1e-6 && v[2].abs() < 1e-6, "{v:?}");
        for _ in 0..100 {
            let v = arrow_motion(&mut rng, [0.0, 3.0, 0.0], [10.0, 1.0, 4.0], 2.0, 3.0);
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            assert!((len - 2.0).abs() < 1e-6);
            assert!(v[0] > 0.0);
        }
        assert_eq!(arrow_motion(&mut rng, [1.0, 1.0, 1.0], [1.0, 1.0, 1.0], 2.0, 0.0), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_leap_goes_at_a_far_target_and_away_from_a_near_one() {
        let mut rng = Rng::new(11);
        let far = multi_shot_leap(&mut rng, [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        assert!(far[0] > 0.5 && far[0] < 1.0 && far[1] == 1.1);
        let near = multi_shot_leap(&mut rng, [0.0, 0.0, 0.0], [2.0, 0.0, 0.0]);
        assert!(near[0] < -0.5 && near[0] > -1.0);
    }

    #[test]
    fn the_swing_reaches_what_is_in_front() {
        let mut rng = Rng::new(13);
        // Yaw 0 looks towards +z.
        assert!(head_angle([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.0]) < 1.0);
        assert!(head_angle([0.0, 0.0, 0.0], 0.0, [0.0, 0.0, -2.0]) > 179.0);
        assert!((head_angle([0.0, 0.0, 0.0], 0.0, [-2.0, 0.0, 0.0]) - 90.0).abs() < 1.0);
        let v = melee_push(&mut rng, [0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.0], 0.0).unwrap();
        assert!(v[2] >= 1.8 && v[2] <= 2.4 + 1e-6 && v[1] > 0.27 && v[0].abs() < 1e-9, "{v:?}");
        assert!(melee_push(&mut rng, [0.0, 0.0, 0.0], 0.0, [0.0, 0.0, -2.0], 0.0).is_none());
        assert!(melee_push(&mut rng, [0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 3.5], 0.0).is_none());
        let up = melee_push(&mut rng, [0.0, 0.0, 0.0], 0.0, [0.0, 0.0, 2.0], 0.9).unwrap();
        assert_eq!(up[1], 0.9);
        let c = constrict_push(&mut rng);
        assert!((1.0..=1.4001).contains(&c[0].abs()) && (0.4..=1.2001).contains(&c[1]) && (1.0..=1.4001).contains(&c[2].abs()));
    }

    #[test]
    fn uuid_words() {
        assert_eq!(uuid_ints(0x0000_0001_0000_0002_FFFF_FFFF_8000_0000), [1, 2, -1, i32::MIN]);
    }

    #[test]
    fn standing_still_only_the_jaw_hangs() {
        let p = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, Anim::None, 0.0);
        for (part, d) in &p.0 {
            if *part == P::Jaw {
                assert!((d.xr - 0.04).abs() < 1e-6);
            } else {
                assert_eq!(*d, Delta::default(), "{part:?}");
            }
        }
        // Looking around turns the head only.
        let p = setup_anim(0.0, 0.0, 0.0, 90.0, 0.0, Anim::None, 0.0);
        assert!((p.get(P::InnerHead).yr - PI / 2.0).abs() < 1e-5);
    }

    #[test]
    fn constrict_opens_then_shuts() {
        assert_eq!(constrict_weights(0.0), (0.0, 0.0));
        assert_eq!(constrict_weights(6.0), (1.0, 0.0));
        assert_eq!(constrict_weights(10.0), (0.0, 1.0));
        let (a, b) = constrict_weights(7.5);
        assert!(a > 0.0 && b > 0.0 && (a + b - 1.0).abs() < 1e-6);
        assert!(constrict_weights(19.9).1 < 0.05);
        assert!((rib_scale(8.0) - 1.6).abs() < 1e-6 && (rib_scale(0.0) - 1.0).abs() < 1e-6);
        // Open: the first right rib swings out (positive yaw); shut: back in.
        let open = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, Anim::ConstrictRibs, 6.0);
        let shut = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, Anim::ConstrictRibs, 10.0);
        assert!(open.get(P::Side1(0, 0)).yr > 0.3 && shut.get(P::Side1(0, 0)).yr < -0.2);
        assert_eq!(open.get(P::Side1(1, 0)).scale, Some(1.6));
        assert!((open.get(P::Base).y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_swing_ends_where_it_began() {
        let end = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, Anim::Melee, 13.999);
        assert!(end.get(P::Arm(R)).yr.abs() < 0.01 && end.get(P::Mid(1)).yr.abs() < 0.01);
        let mid = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, Anim::Melee, 6.0);
        assert!((mid.get(P::Arm(R)).yr + 2.416_61).abs() < 1e-5);
        // The two halves of the wind-up meet at tick 3.
        let before = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, Anim::Melee, 2.999).get(P::Arm(R));
        let after = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, Anim::Melee, 3.0).get(P::Arm(R));
        assert!((before.yr - after.yr).abs() < 0.01 && (before.zr - after.zr).abs() < 0.01);
    }

    #[test]
    fn the_shot_is_continuous_and_ends_at_rest() {
        for anim in [Anim::Shoot, Anim::MultiShot] {
            let mut last = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, anim, 0.0);
            let mut t = 0.0f32;
            while t < anim.duration() as f32 {
                t += 0.05;
                let now = setup_anim(0.0, 0.0, 0.0, 0.0, 0.0, anim, t);
                for part in [P::Arm(R), P::Arm(L), P::ArmInner(R), P::ArmInner(L), P::ArmLower(R), P::ArmLower(L), P::Mid(0)] {
                    let (a, b) = (last.get(part), now.get(part));
                    assert!(
                        (a.xr - b.xr).abs() < 0.2 && (a.yr - b.yr).abs() < 0.2 && (a.zr - b.zr).abs() < 0.2,
                        "{anim:?} jumps at {t} on {part:?}: {a:?} -> {b:?}"
                    );
                }
                last = now;
            }
            assert!(last.get(P::Arm(R)).yr.abs() < 0.05 && last.get(P::ArmInner(L)).xr.abs() < 0.05);
        }
    }

    #[test]
    fn paths_follow_the_tree() {
        assert_eq!(P::Base.path(), ["base"]);
        assert_eq!(P::Mid(1).path(), ["base", "pelvis", "waist", "middle1", "middle2"]);
        assert_eq!(P::Side2(2, 1).path().last().unwrap(), "side223");
        assert_eq!(P::Jaw.path().len(), 10);
        assert_eq!(P::ArmLower(L).path().last().unwrap(), "left_arm_lower");
        assert_eq!(P::LegLowerInner(R).path(), ["base", "pelvis", "right_leg", "right_leg_inner", "right_leg_lower", "right_leg_lower_inner"]);
    }
}
