//! The server half: plays out the attacks the goals start (`SlamGroundGoal`,
//! `ThrowAttackGoal`, `RoarGoal`), the seismic waves and zombie resurrections
//! they leave (`MutantZombie.updateMeleeGrounds`, `ZombieResurrection`), the
//! three lives (`die`, `tickDeath`), the Hulk Hammer's waves
//! (`HulkHammerItem.useOn`, `PlayerEventsHandler.onEndPlayerTick`) and Chemical X
//! (`ChemicalXMobEffect`, `SkullSpirit`). Translated from Mutant Monsters
//! (AGPL-3.0, see NOTICE.md); upstream's own sounds are All Rights Reserved,
//! so vanilla sound events stand in for them.
use crate::brain::{self, Brain};
use crate::creeper;
use crate::skeleton;
use crate::logic::{self, Anim, Cell, Lethal, Lives, Resurrection, Rng, Wave};
use crate::net::{self, AnimMessage, PushMessage};
use horizon_client::mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, Compound, EntityTickListenedEvent,
    EntityTickPostEvent, Flow, GameRuleValue, HearsProjectileImpactsEvent, Holder, Host,
    LevelTickPostEvent, LivingDeathEvent, LivingIncomingDamageEvent, LivingConversionPreEvent,
    NativeEffect, NativeInventoryKind, NativeServerPlayer, ParticleOptions, ParticleType,
    ProjectileImpactEvent, ServerTickEvent, ServerTickPhase, Tag, UseItemOnEvent,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

/// `MM_TRACE=1` in the server's environment logs what the zombies do (for
/// smoke tests on a scratch server).
fn trace_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("MM_TRACE").is_some())
}

/// A trace line from another module (`creeper.rs`).
pub fn trace_line(args: std::fmt::Arguments<'_>) {
    if trace_on() {
        eprintln!("[mutant_monsters] {args}");
    }
}

macro_rules! trace {
    ($($arg:tt)*) => {
        if trace_on() {
            eprintln!("[mutant_monsters] {}", format_args!($($arg)*));
        }
    };
}

const HOSTILE: usize = 5;
const NEUTRAL: usize = 6;
const BLOCKS: usize = 4;
/// Players this far from a zombie are told its animations.
const VIEW: f64 = 96.0;

/// Server ticks, counted by `server_tick`.
static TICKS: AtomicU64 = AtomicU64::new(0);

fn now() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// A zombie's lives, saved with it (`Lives` / `VanishTime` upstream).
static LIVES: AttachmentType<Lives> = AttachmentType {
    namespace: "mutant_monsters",
    name: "lives",
    default: Lives::default,
    codec: Some(AttachmentCodec {
        encode: |l| {
            let mut c = Compound::new();
            c.insert("lives".into(), Tag::Byte(l.lives as i8));
            c.insert("down".into(), Tag::Int(l.down as i32));
            Ok(c)
        },
        decode: |c| {
            let lives = match c.get(&"lives".into()) {
                Some(Tag::Byte(b)) => (*b).clamp(0, 127) as u8,
                _ => return Err(AttachmentError::Decode),
            };
            let down = match c.get(&"down".into()) {
                Some(Tag::Int(d)) => (*d).max(0) as u32,
                _ => 0,
            };
            Ok(Lives { lives, down })
        },
    }),
    copy_on_death: false,
    sync: false,
};

/// What only the server keeps of a zombie between ticks.
struct Runtime {
    lives: Lives,
    waves: VecDeque<Wave>,
    resurrections: Vec<Resurrection>,
    rng: Rng,
    slam_dir: [f64; 2],
    throw_hit: bool,
    throw_finished: bool,
    throw_finish: i32,
    /// The target this attack is aimed at, settled when it started.
    target: Option<Resolved>,
    last_seen: u64,
}

fn runtimes() -> &'static Mutex<HashMap<(String, u32), Runtime>> {
    static RT: OnceLock<Mutex<HashMap<(String, u32), Runtime>>> = OnceLock::new();
    RT.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A target found in the world: a mob, or a player (who is pushed through
/// their own client).
#[derive(Clone, Copy, Debug)]
struct Resolved {
    entity: u32,
    player: Option<u128>,
    pos: [f64; 3],
}

// --- hooks ----------------------------------------------------------------------

pub fn entity_tick_listened(_host: &mut dyn Host, event: &mut EntityTickListenedEvent) {
    event.listened = true;
}

pub fn hears_projectile_impacts(event: &mut HearsProjectileImpactsEvent) {
    event.hears = true;
}

/// The zombie template turns into a drowned underwater: a Mutant Zombie
/// doesn't.
pub fn living_conversion_pre(event: &mut LivingConversionPreEvent<'_>) -> Flow {
    let kind = logic::kind_id(event.kind);
    if kind == logic::MUTANT_ZOMBIE || kind == creeper::MUTANT_CREEPER || kind == skeleton::MUTANT_SKELETON {
        Flow::Cancel
    } else {
        Flow::Continue
    }
}

/// `MutantZombie.hurtServer`: nothing hurts a downed zombie (it is dead to
/// the game upstream), fire on a downed zombie takes its last lives
/// (`igniteForTicks`), and a thrown target can't hit back mid-throw.
pub fn living_incoming_damage(event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
    if logic::kind_id(event.kind) == creeper::MUTANT_CREEPER {
        return creeper::living_incoming_damage(event.entity, event.damage_type, event.causing, event.direct, event.amount);
    }
    if logic::kind_id(event.kind) == skeleton::MUTANT_SKELETON {
        return skeleton::living_incoming_damage(event.entity, event.causing);
    }
    if logic::kind_id(event.kind) != logic::MUTANT_ZOMBIE {
        return Flow::Continue;
    }
    let down = brain::downed().lock().map(|s| s.contains(&event.entity)).unwrap_or(false);
    if down {
        if is_fire(event.damage_type) {
            if let Ok(mut set) = brain::ignited().lock() {
                set.insert(event.entity);
            }
        }
        return Flow::Cancel;
    }
    let thrown = brain::throwing().lock().ok().and_then(|m| m.get(&event.entity).copied());
    if thrown.is_some_and(|t| t == event.causing && t > 0) {
        return Flow::Cancel;
    }
    // Upstream's Mutant Zombie is no Zombie: drowning doesn't hurt it either.
    if event.damage_type == "minecraft:drown" {
        return Flow::Cancel;
    }
    Flow::Continue
}

fn is_fire(damage_type: &str) -> bool {
    matches!(
        damage_type,
        "minecraft:in_fire" | "minecraft:on_fire" | "minecraft:lava" | "minecraft:campfire"
            | "minecraft:hot_floor" | "minecraft:fireball" | "minecraft:unattributed_fireball"
    )
}

/// `MutantZombie.die`: with lives left the zombie only falls down.
pub fn living_death(host: &mut dyn Host, event: &mut LivingDeathEvent<'_>) -> Flow {
    if logic::kind_id(event.kind) == creeper::MUTANT_CREEPER {
        return creeper::living_death(host, event.entity, event.attacker);
    }
    if logic::kind_id(event.kind) == skeleton::MUTANT_SKELETON {
        return skeleton::living_death(host, event.entity, event.attacker);
    }
    if logic::kind_id(event.kind) != logic::MUTANT_ZOMBIE {
        return Flow::Continue;
    }
    let id = event.entity;
    let burning = host
        .entity(id)
        .and_then(|e| host.entity_fire_ticks(e))
        .is_some_and(|t| t > 0);
    let forced = matches!(event.damage_type, "minecraft:generic_kill" | "minecraft:out_of_world");
    let mut lives = load_lives(host, id);
    let outcome = lives.lethal(burning, forced);
    trace!("zombie {id} lethal hit ({}, burning {burning}): {outcome:?}, lives left {}", event.damage_type, lives.lives);
    save_lives(host, id, lives);
    with_runtime_any(id, |rt| rt.lives = lives);
    match outcome {
        Lethal::GoDown => {
            set_down(id, true);
            brain::with(id, |b| {
                b.down = true;
                b.anim = Anim::None;
                b.anim_tick = 0;
            });
            if let Some(pos) = host.entity(id).and_then(|e| host.entity_position(e)) {
                let _ = host.play_sound("minecraft:entity.zombie.death", pos, HOSTILE, 1.5, 0.5);
                broadcast(host, id, pos, Anim::Death, 0, 0);
            }
            Flow::Cancel
        }
        Lethal::StayDown => Flow::Cancel,
        Lethal::Die => {
            set_down(id, false);
            // `Enemy.XP_REWARD_HUGE` (50) for a player's kill: the zombie class
            // it copies gives 5 of it.
            let by_player = event.attacker > 0
                && host.server_players().iter().any(|p| p.entity_id == event.attacker as i32);
            if by_player {
                if let Some(pos) = host.entity(id).and_then(|e| host.entity_position(e)) {
                    let mut orb = Compound::new();
                    orb.insert("Value".into(), Tag::Short(45));
                    let dim = runtimes().lock().ok().and_then(|m| m.keys().find(|(_, i)| *i == id).map(|(d, _)| d.clone()));
                    if let Some(dim) = dim {
                        let _ = host.summon(&dim, "minecraft:experience_orb", [pos[0], pos[1] + 0.5, pos[2]], orb);
                    }
                }
            }
            if let Ok(mut m) = brain::brains().lock() {
                m.remove(&id);
            }
            Flow::Continue
        }
    }
}

/// One tick of a Mutant Zombie, after the game ticked it.
pub fn entity_tick_post(host: &mut dyn Host, event: &mut EntityTickPostEvent<'_>) {
    if logic::kind_id(event.kind) == creeper::MUTANT_CREEPER {
        creeper::entity_tick_post(host, event.dimension, event.entity, now());
        return;
    }
    if logic::kind_id(event.kind) == skeleton::MUTANT_SKELETON {
        skeleton::entity_tick_post(host, event.dimension, event.entity, now());
        return;
    }
    if logic::kind_id(event.kind) != logic::MUTANT_ZOMBIE {
        return;
    }
    let dim = event.dimension.to_owned();
    let id = event.entity;
    let Some(brain) = brain::get(id) else {
        // Its goals haven't run yet (or it is a client copy).
        return;
    };
    let key = (dim.clone(), id);
    let first = runtimes().lock().map(|m| !m.contains_key(&key)).unwrap_or(false);
    if first {
        let lives = load_lives(host, id);
        if let Ok(mut map) = runtimes().lock() {
            map.insert(
                key.clone(),
                Runtime {
                    lives,
                    waves: VecDeque::new(),
                    resurrections: Vec::new(),
                    rng: Rng::new(u64::from(id) ^ now()),
                    slam_dir: [0.0, 0.0],
                    throw_hit: false,
                    throw_finished: false,
                    throw_finish: -1,
                    target: None,
                    last_seen: now(),
                },
            );
        }
        if let Some(e) = host.entity(id) {
            let mut nbt = Compound::new();
            nbt.insert("IsBaby".into(), Tag::Byte(0));
            nbt.insert("CanPickUpLoot".into(), Tag::Byte(0));
            nbt.insert("CanBreakDoors".into(), Tag::Byte(0));
            let _ = host.entity_merge_nbt(e, nbt);
        }
        if lives.is_down() {
            set_down(id, true);
            brain::with(id, |b| b.down = true);
        }
    }
    let players = host.server_players();
    let Ok(mut map) = runtimes().lock() else { return };
    let Some(rt) = map.get_mut(&key) else { return };
    rt.last_seen = now();
    let mut brain = brain;
    let mut kill = false;
    if rt.lives.is_down() {
        kill = tick_down(host, &dim, id, &brain, rt, &players);
    } else if brain.anim != Anim::None {
        tick_attack(host, &dim, id, &mut brain, rt, &players);
    }
    // One wave a tick (`updateMeleeGrounds`).
    if let Some(wave) = rt.waves.pop_front() {
        let griefing = mob_griefing(host, &dim);
        affect_blocks(host, &dim, &wave, griefing);
        wave_hits(host, &dim, id, &brain, &wave, rt, &players);
    }
    tick_resurrections(host, &dim, rt);
    let resurrecting = !rt.resurrections.is_empty();
    // `tick()`: two health back every five seconds at night.
    if !rt.lives.is_down()
        && brain.age % 100 == 0
        && logic::is_night(host.day_time())
        && brain.health > 0.0
        && brain.health < logic::MAX_HEALTH
    {
        let _ = host.set_mob_health(&dim, id, (brain.health + 2.0).min(logic::MAX_HEALTH));
    }
    drop(map);
    if kill {
        // `/kill`'s path: `living_death` sees no lives left and lets it die.
        // Called with no lock held: the death hook takes them again.
        let _ = host.kill_mob(&dim, id);
        return;
    }
    brain::with(id, |b| {
        b.anim = brain.anim;
        b.anim_tick = brain.anim_tick;
        b.resurrecting = resurrecting;
    });
}

/// Once a server tick: count it, and forget zombies that stopped ticking
/// (dead, unloaded or removed).
pub fn server_tick(_host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
    if !matches!(event.phase, ServerTickPhase::ServerTickStart { .. }) {
        return;
    }
    let t = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    if t % 200 != 0 {
        return;
    }
    let mut gone = Vec::new();
    if let Ok(mut map) = runtimes().lock() {
        map.retain(|(_, id), rt| {
            let keep = t.saturating_sub(rt.last_seen) < 400;
            if !keep {
                gone.push(*id);
            }
            keep
        });
    }
    if let Ok(mut b) = brain::brains().lock() {
        for id in &gone {
            b.remove(id);
        }
    }
    if let Ok(mut d) = brain::downed().lock() {
        for id in &gone {
            d.remove(&(*id as i32));
        }
    }
    if let Ok(mut c) = conversions().lock() {
        c.retain(|c| t.saturating_sub(c.started) < 2000);
    }
    creeper::forget_stale(t);
    skeleton::forget_stale(t);
}

// --- lives ----------------------------------------------------------------------

fn load_lives(host: &mut dyn Host, id: u32) -> Lives {
    let cached = runtimes()
        .lock()
        .ok()
        .and_then(|m| m.iter().find(|((_, i), _)| *i == id).map(|(_, rt)| rt.lives));
    cached.unwrap_or_else(|| LIVES.get(host, Holder::Entity(id)).unwrap_or_default())
}

fn save_lives(host: &mut dyn Host, id: u32, lives: Lives) {
    let _ = LIVES.set(host, Holder::Entity(id), lives);
}

fn with_runtime_any(id: u32, f: impl FnOnce(&mut Runtime)) {
    if let Ok(mut m) = runtimes().lock() {
        if let Some((_, rt)) = m.iter_mut().find(|((_, i), _)| *i == id) {
            f(rt);
        }
    }
}

fn set_down(id: u32, down: bool) {
    if let Ok(mut set) = brain::downed().lock() {
        if down {
            set.insert(id as i32);
        } else {
            set.remove(&(id as i32));
        }
    }
    if !down {
        if let Ok(mut set) = brain::ignited().lock() {
            set.remove(&(id as i32));
        }
    }
}

/// `tickDeath` with lives left: lie there for 140 ticks, then stand up
/// with a quarter of the health. Fire ends it for good.
fn tick_down(
    host: &mut dyn Host,
    dim: &str,
    id: u32,
    brain: &Brain,
    rt: &mut Runtime,
    players: &[NativeServerPlayer],
) -> bool {
    let ignited = brain::ignited().lock().map(|s| s.contains(&(id as i32))).unwrap_or(false);
    let burning = host.entity(id).and_then(|e| host.entity_fire_ticks(e)).is_some_and(|t| t > 0);
    if ignited || burning {
        trace!("zombie {id} burns while down: dies");
        rt.lives.ignite();
        save_lives(host, id, rt.lives);
        set_down(id, false);
        if let Some(e) = host.entity(id) {
            let _ = host.entity_set_fire(e, 160);
        }
        return true;
    }
    if rt.lives.tick_down() {
        trace!("zombie {id} gets up, lives left {}", rt.lives.lives);
        set_down(id, false);
        let _ = host.set_mob_health(dim, id, logic::revive_health());
        brain::with(id, |b| {
            b.down = false;
            b.anim = Anim::None;
            b.anim_tick = 0;
        });
        save_lives(host, id, rt.lives);
        broadcast_to(host, players, id, brain.pos, Anim::None, 0, 0);
        let _ = host.play_sound("minecraft:entity.zombie.ambient", brain.pos, HOSTILE, 2.0, 0.5);
        return false;
    }
    // Late joiners see it lying there; the save keeps how long it has.
    if rt.lives.down % 20 == 0 {
        save_lives(host, id, rt.lives);
        broadcast_to(host, players, id, brain.pos, Anim::Death, rt.lives.down, 0);
    }
    let _ = dim;
    false
}

// --- attacks ---------------------------------------------------------------------

/// The target the goals saw, found in the world.
fn resolve(host: &mut dyn Host, dim: &str, brain: &Brain, players: &[NativeServerPlayer]) -> Option<Resolved> {
    let t = brain.target?;
    if let Some(mob) = t.mob {
        let pos = host.entity(mob).and_then(|e| host.entity_position(e)).unwrap_or(t.pos);
        return Some(Resolved { entity: mob, player: None, pos });
    }
    players
        .iter()
        .filter(|p| p.dimension == dim && p.entity_id > 0 && p.health > 0.0)
        .map(|p| (logic::dist_sqr(p.position, t.pos), p))
        .filter(|(d, _)| *d < 16.0)
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, p)| Resolved { entity: p.entity_id as u32, player: Some(p.uuid), pos: p.position })
}

/// Where a resolved target is now.
fn refresh(host: &mut dyn Host, players: &[NativeServerPlayer], t: &Resolved) -> Option<Resolved> {
    match t.player {
        Some(uuid) => players
            .iter()
            .find(|p| p.uuid == uuid && p.health > 0.0 && matches!(p.game_mode, 0 | 2))
            .map(|p| Resolved { pos: p.position, ..*t }),
        None => host
            .entity(t.entity)
            .and_then(|e| host.entity_position(e))
            .map(|pos| Resolved { pos, ..*t }),
    }
}

fn push(host: &mut dyn Host, dim: &str, t: &Resolved, v: [f64; 3]) {
    match t.player {
        Some(uuid) => {
            let msg = PushMessage([v[0] as f32, v[1] as f32, v[2] as f32]);
            let _ = host.net_send_to_player(uuid, net::PUSH, &msg.encode());
        }
        None => {
            let _ = host.push_mob(dim, t.entity, v);
        }
    }
}

fn tick_attack(
    host: &mut dyn Host,
    dim: &str,
    id: u32,
    brain: &mut Brain,
    rt: &mut Runtime,
    players: &[NativeServerPlayer],
) {
    let t = brain.anim_tick;
    let pos = brain.pos;
    if t == 0 {
        rt.target = resolve(host, dim, brain, players);
        trace!("zombie {id} starts {:?} at {:?}, target {:?}", brain.anim, pos, rt.target);
        rt.throw_hit = false;
        rt.throw_finished = false;
        rt.throw_finish = -1;
        broadcast_to(host, players, id, pos, brain.anim, 0, 0);
    } else if t % 20 == 0 {
        broadcast_to(host, players, id, pos, brain.anim, t, throw_flags(rt));
    }
    let target = rt.target.and_then(|r| refresh(host, players, &r));
    let pitch = 0.8 + rt.rng.float() * 0.4;
    let done;
    match brain.anim {
        Anim::Slam => {
            if t == 0 {
                let _ = host.play_sound("minecraft:entity.ravager.attack", pos, HOSTILE, 0.6, pitch);
            }
            if t == 8 {
                rt.slam_dir = target.map(|r| logic::flat_dir(pos, r.pos)).unwrap_or(rt.slam_dir);
            }
            if t == 12 {
                let (x1, z1, x2, z2) = logic::wave_line(pos, rt.slam_dir, 2.0, 8.0);
                let waves = make_waves(host, dim, x1, z1, x2, z2, pos[1].floor() as i32, &mut rt.rng);
                trace!("zombie {id} slams: {} waves from ({x1}, {z1}) to ({x2}, {z2})", waves.len());
                rt.waves.extend(waves);
                let _ = host.play_sound("minecraft:entity.generic.explode", pos, HOSTILE, 0.5, pitch);
            }
            done = t + 1 >= Anim::Slam.duration();
        }
        Anim::Throw => {
            let Some(target) = target else {
                end_throw(id);
                finish(host, players, id, brain, rt);
                return;
            };
            if let Ok(mut m) = brain::throwing().lock() {
                m.insert(id as i32, target.entity as i32);
            }
            if t == 0 {
                push(host, dim, &target, logic::throw_launch(pos, target.pos));
            }
            if t == Anim::Throw.duration() {
                let _ = host.push_mob(dim, id, logic::throw_leap(pos, target.pos));
            } else if t > Anim::Throw.duration() {
                if !rt.throw_hit && logic::throw_reaches(pos, target.pos) {
                    rt.throw_hit = true;
                    trace!("zombie {id} throw hits {}", target.entity);
                    let _ = host.hurt_entity(dim, target.entity, logic::ATTACK_DAMAGE, "minecraft:mob_attack", Some(id));
                    push(host, dim, &target, logic::throw_slam_down(pos, target.pos));
                    let _ = host.play_sound("minecraft:entity.zombie.attack_iron_door", pos, HOSTILE, 0.3, pitch);
                    broadcast_to(host, players, id, pos, Anim::Throw, t, throw_flags(rt));
                }
                // A few ticks in the air first: the leap is applied after the
                // tick the goals read `onGround` in.
                if t >= Anim::Throw.duration() + 3 && (brain.on_ground || brain.in_fluid) && !rt.throw_finished {
                    rt.throw_finished = true;
                    rt.throw_finish = 0;
                    broadcast_to(host, players, id, pos, Anim::Throw, t, throw_flags(rt));
                }
                if rt.throw_finish >= 0 {
                    rt.throw_finish += 1;
                }
            }
            done = rt.throw_finish >= 10 || t > 200;
            if done {
                end_throw(id);
            }
        }
        Anim::Roar => {
            if t == 10 {
                let pitch = 0.7 + rt.rng.float() * 0.2;
                let _ = host.play_sound("minecraft:entity.ravager.roar", pos, HOSTILE, 3.0, pitch);
                trace!("zombie {id} roars");
                roar_blast(host, dim, id, pos, rt, players);
            }
            if (20..80).contains(&t) && t % 10 == 0 {
                let (x, y, z) = logic::roar_spot(&mut rt.rng, pos);
                let day = !logic::is_night(host.day_time());
                if let Some(gy) = ground(host, dim, x, y, z, 4, true) {
                    // `getSuitableGround`'s daylight test: by day a spot is
                    // refused two times in three (the sky can't be asked here).
                    if !day || rt.rng.int(3) == 0 {
                        let r = Resurrection::new(&mut rt.rng, [x, gy, z]);
                        rt.resurrections.push(r);
                    }
                }
            }
            done = t + 1 >= Anim::Roar.duration();
        }
        Anim::None | Anim::Death => done = true,
    }
    if done {
        finish(host, players, id, brain, rt);
    } else {
        brain.anim_tick += 1;
    }
}

fn end_throw(id: u32) {
    if let Ok(mut m) = brain::throwing().lock() {
        m.remove(&(id as i32));
    }
}

fn finish(host: &mut dyn Host, players: &[NativeServerPlayer], id: u32, brain: &mut Brain, rt: &mut Runtime) {
    brain.anim = Anim::None;
    brain.anim_tick = 0;
    rt.target = None;
    broadcast_to(host, players, id, brain.pos, Anim::None, 0, 0);
}

fn throw_flags(rt: &Runtime) -> u8 {
    (if rt.throw_hit { net::THROW_HIT } else { 0 }) | (if rt.throw_finished { net::THROW_FINISHED } else { 0 })
}

/// `RoarGoal.tick` at animation tick 10: everything but zombies within 14
/// blocks is pushed away and hurt for 2 or 3.
fn roar_blast(host: &mut dyn Host, dim: &str, id: u32, pos: [f64; 3], rt: &mut Runtime, players: &[NativeServerPlayer]) {
    let (hw, hh) = (logic::WIDTH / 2.0 + 12.0, 8.0);
    let min = [pos[0] - hw, pos[1] - hh, pos[2] - hw];
    let max = [pos[0] + hw, pos[1] + logic::HEIGHT + hh, pos[2] + hw];
    for p in players.iter().filter(|p| p.dimension == dim && matches!(p.game_mode, 0 | 2) && p.entity_id > 0) {
        if logic::dist_sqr(p.position, pos) <= 196.0 {
            let r = Resolved { entity: p.entity_id as u32, player: Some(p.uuid), pos: p.position };
            push(host, dim, &r, logic::roar_push(pos, p.position));
            let amount = (2 + rt.rng.int(2)) as f32;
            let _ = host.hurt_entity(dim, r.entity, amount, "mutant_monsters:piercing_mob_attack", Some(id));
        }
    }
    for (mob, at) in mobs_in(host, dim, id, players, min, max) {
        if logic::dist_sqr(at, pos) <= 196.0 {
            let _ = host.push_mob(dim, mob, logic::roar_push(pos, at));
            let amount = (2 + rt.rng.int(2)) as f32;
            let _ = host.hurt_entity(dim, mob, amount, "mutant_monsters:piercing_mob_attack", Some(id));
        }
    }
}

/// The living mobs in a box that a Mutant Zombie may harm (no zombies, not
/// itself, not players), with where they are.
fn mobs_in(
    host: &mut dyn Host,
    dim: &str,
    except: u32,
    players: &[NativeServerPlayer],
    min: [f64; 3],
    max: [f64; 3],
) -> Vec<(u32, [f64; 3])> {
    let ids = host.entities_in(dim, min, max);
    let mut out = Vec::new();
    for id in ids {
        if id == except || players.iter().any(|p| p.entity_id == id as i32) {
            continue;
        }
        let Some(e) = host.entity(id) else { continue };
        if host.living(e).is_none() {
            continue;
        }
        let Some(kind) = host.entity_kind(e) else { continue };
        if logic::is_zombie(&kind) || kind == "minecraft:armor_stand" {
            continue;
        }
        if let Some(at) = host.entity_position(e) {
            out.push((id, at));
        }
    }
    out
}

// --- seismic waves -----------------------------------------------------------------

fn cell(host: &dyn Host, dim: &str, pos: [i32; 3]) -> Cell {
    host.level(dim)
        .and_then(|l| host.level_block_state(l, pos))
        .map(|s| logic::classify(&s))
        .unwrap_or(Cell::Solid)
}

fn block_at(host: &dyn Host, dim: &str, pos: [i32; 3]) -> Option<String> {
    host.level(dim).and_then(|l| host.level_block_state(l, pos))
}

fn ground(host: &dyn Host, dim: &str, x: i32, y: i32, z: i32, range: i32, check_day: bool) -> Option<i32> {
    logic::suitable_ground(|yy| cell(host, dim, [x, yy, z]), y, range, check_day)
}

#[allow(clippy::too_many_arguments)]
fn make_waves(host: &dyn Host, dim: &str, x1: i32, z1: i32, x2: i32, z2: i32, y: i32, rng: &mut Rng) -> Vec<Wave> {
    logic::create_waves(x1, z1, x2, z2, y, |x, yy, z| ground(host, dim, x, yy, z, 3, false), || rng.int(2) == 0)
}

fn mob_griefing(host: &dyn Host, dim: &str) -> bool {
    host.level(dim)
        .and_then(|l| host.level_game_rule(l, "minecraft:mob_griefing"))
        .map(|v| matches!(v, GameRuleValue::Bool(true)))
        .unwrap_or(true)
}

/// `SeismicWave.affectBlocks`: grass and paths turn to dirt and loose plants
/// are knocked off (when allowed), and the block's dust flies up.
fn affect_blocks(host: &mut dyn Host, dim: &str, wave: &Wave, allowed: bool) {
    if !wave.terrain {
        return;
    }
    let Some(state) = block_at(host, dim, wave.pos) else { return };
    let name = logic::block_name(&state).to_owned();
    let above = [wave.pos[0], wave.pos[1] + 1, wave.pos[2]];
    if allowed {
        if logic::turns_into_dirt(&name) {
            if let Some(dirt) = host.default_state("minecraft:dirt") {
                let _ = host.set_block(dim, wave.pos, dirt, 3);
            }
        }
        if let Some(up) = block_at(host, dim, above) {
            if logic::classify(&up) == Cell::Passable {
                if let Some(air) = host.default_state("minecraft:air") {
                    let _ = host.set_block(dim, above, air, 3);
                }
            }
        }
    }
    if matches!(logic::classify(&state), Cell::Fluid { .. }) {
        let at = [f64::from(above[0]) + 0.5, f64::from(above[1]), f64::from(above[2]) + 0.5];
        let _ = host.send_particles(&ParticleOptions::simple(ParticleType::Splash), None, at, [0.3, 0.1, 0.3], 0.1, 12);
    } else if let Some(id) = host.default_state(&name) {
        let at = [f64::from(above[0]) + 0.5, f64::from(above[1]) + 0.1, f64::from(above[2]) + 0.5];
        let _ = host.send_particles(&ParticleOptions::simple(ParticleType::Block), Some(id), at, [0.25, 0.1, 0.25], 0.1, 16);
    }
}

/// `updateMeleeGrounds`: what stands on the wave is hurt (9 to 12 for the
/// first, 6 to 8 after), sometimes starved, and pushed away.
fn wave_hits(
    host: &mut dyn Host,
    dim: &str,
    id: u32,
    brain: &Brain,
    wave: &Wave,
    rt: &mut Runtime,
    players: &[NativeServerPlayer],
) {
    let (min, max) = logic::wave_box(wave, rt.rng.double() * 0.75);
    let damage_type = "mutant_monsters:mutant_zombie_seismic_wave";
    for p in players.iter().filter(|p| p.dimension == dim && matches!(p.game_mode, 0 | 2) && p.entity_id > 0) {
        if !player_in(p, min, max) {
            continue;
        }
        let amount = logic::wave_damage(&mut rt.rng, wave.initial);
        let hurt = host.hurt_entity(dim, p.entity_id as u32, amount, damage_type, Some(id));
        trace!("zombie {id} wave at {:?} hits player {} for {amount}: {hurt}", wave.pos, p.name);
        if hurt && rt.rng.int(5) == 0 {
            let _ = host.add_effect(
                p.uuid,
                &NativeEffect { id: "minecraft:hunger".to_owned(), amplifier: 1, duration: 160, ambient: false, particles: true },
            );
        }
        let r = Resolved { entity: p.entity_id as u32, player: Some(p.uuid), pos: p.position };
        push(host, dim, &r, logic::wave_push(brain.pos, p.position));
    }
    for (mob, at) in mobs_in(host, dim, id, players, min, max) {
        let amount = logic::wave_damage(&mut rt.rng, wave.initial);
        let _ = host.hurt_entity(dim, mob, amount, damage_type, Some(id));
        let _ = host.push_mob(dim, mob, logic::wave_push(brain.pos, at));
    }
}

/// Whether a player's box (0.6 x 1.8) meets the box from `min` to `max`.
fn player_in(p: &NativeServerPlayer, min: [f64; 3], max: [f64; 3]) -> bool {
    let [x, y, z] = p.position;
    x + 0.3 > min[0] && x - 0.3 < max[0] && y + 1.8 > min[1] && y < max[1] && z + 0.3 > min[2] && z - 0.3 < max[2]
}

// --- zombie resurrection ---------------------------------------------------------------

/// `ZombieResurrection.update` for each spot: dust now and then, and a
/// zombie when its time comes.
fn tick_resurrections(host: &mut dyn Host, dim: &str, rt: &mut Runtime) {
    let mut spots = std::mem::take(&mut rt.resurrections);
    spots.retain_mut(|r| {
        let below = block_at(host, dim, r.pos);
        let above = [r.pos[0], r.pos[1] + 1, r.pos[2]];
        let above_state = block_at(host, dim, above);
        let solid_below = below.as_deref().is_some_and(|s| logic::classify(s) != Cell::Air);
        let open_above = above_state.as_deref().is_some_and(|s| logic::classify(s) == Cell::Air);
        if !solid_below || !open_above {
            return false;
        }
        let name = below.as_deref().map(logic::block_name).unwrap_or("minecraft:dirt").to_owned();
        let at = [f64::from(above[0]) + 0.5, f64::from(above[1]), f64::from(above[2]) + 0.5];
        if rt.rng.int(15) == 0 {
            dust(host, &name, at);
        }
        r.tick -= 1;
        if r.tick > 0 {
            return true;
        }
        let in_water = matches!(above_state.as_deref().map(logic::classify), Some(Cell::Fluid { lava: false }));
        let day = !logic::is_night(host.day_time());
        let kind = logic::resurrected_kind(&mut rt.rng, in_water, day);
        let mut nbt = Compound::new();
        let health = 20.0 * (0.6 + 0.4 * rt.rng.float());
        nbt.insert("Health".into(), Tag::Float(health.to_bits()));
        let summoned = host.summon(dim, kind, at, nbt);
        trace!("resurrection at {:?}: {kind} {summoned:?}", r.pos);
        dust(host, &name, at);
        let _ = host.play_sound("minecraft:entity.zombie.ambient", at, HOSTILE, 1.0, 1.0);
        false
    });
    rt.resurrections = spots;
}

fn dust(host: &mut dyn Host, block: &str, at: [f64; 3]) {
    if let Some(id) = host.default_state(block) {
        let _ = host.send_particles(&ParticleOptions::simple(ParticleType::Block), Some(id), at, [0.3, 0.1, 0.3], 0.05, 20);
    }
}

// --- animation messages -------------------------------------------------------------

fn broadcast(host: &mut dyn Host, id: u32, pos: [f64; 3], anim: Anim, elapsed: u32, flags: u8) {
    let players = host.server_players();
    broadcast_to(host, &players, id, pos, anim, elapsed, flags);
}

fn broadcast_to(
    host: &mut dyn Host,
    players: &[NativeServerPlayer],
    id: u32,
    pos: [f64; 3],
    anim: Anim,
    elapsed: u32,
    flags: u8,
) {
    let msg = AnimMessage { entity: id, anim: anim.id(), elapsed: elapsed.min(u32::from(u16::MAX)) as u16, flags };
    let bytes = msg.encode();
    for p in players {
        if logic::dist_sqr(p.position, pos) <= VIEW * VIEW {
            let _ = host.net_send_to_player(p.uuid, net::ANIM, &bytes);
        }
    }
}

// --- the Hulk Hammer -------------------------------------------------------------------

/// The waves each player's hammer sent, played one a tick
/// (`SEISMIC_WAVE_ATTACHMENT_TYPE`, at most 64).
fn player_waves() -> &'static Mutex<HashMap<u128, VecDeque<Wave>>> {
    static W: OnceLock<Mutex<HashMap<u128, VecDeque<Wave>>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The hammer's 25-tick cooldown, by player (Horizon's server hooks can't set
/// an item cooldown) and the uses whose durability is still to be taken.
fn hammer_state() -> &'static Mutex<(HashMap<u128, u64>, Vec<(u128, usize)>)> {
    static S: OnceLock<Mutex<(HashMap<u128, u64>, Vec<(u128, usize)>)>> = OnceLock::new();
    S.get_or_init(|| Mutex::new((HashMap::new(), Vec::new())))
}

/// `HulkHammerItem.useOn`: on top of a block, send a line of waves the way
/// the player faces.
pub fn use_item_on(host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
    if &*event.stack.item != logic::HULK_HAMMER || event.face != [0, 1, 0] {
        return Flow::Continue;
    }
    let Some(player) = host.acting_player() else { return Flow::Continue };
    {
        let Ok(mut s) = hammer_state().lock() else { return Flow::Continue };
        if s.0.get(&player.uuid).is_some_and(|until| now() < *until) {
            return Flow::Cancel;
        }
        s.0.insert(player.uuid, now() + 25);
        s.1.push((player.uuid, usize::from(player.selected_slot)));
    }
    let dir = logic::yaw_dir(player.yaw);
    let (x1, z1, x2, z2) = logic::wave_line(player.position, dir, 1.5, 8.0);
    let mut rng = Rng::new(player.uuid as u64 ^ now());
    let waves = make_waves(host, &player.dimension, x1, z1, x2, z2, player.position[1].floor() as i32, &mut rng);
    trace!("hammer of {} sends {} waves", player.name, waves.len());
    if let Ok(mut map) = player_waves().lock() {
        let list = map.entry(player.uuid).or_default();
        list.extend(waves);
        while list.len() > 64 {
            list.pop_front();
        }
    }
    let at = [f64::from(event.pos[0]) + 0.5, f64::from(event.pos[1]) + 1.0, f64::from(event.pos[2]) + 0.5];
    let _ = host.play_sound("minecraft:entity.generic.explode", at, BLOCKS, 0.8, 0.8 + rng.float() * 0.4);
    Flow::Cancel
}

/// The players of level `dim` with hammer waves or wear to apply.
fn hammer_players(host: &mut dyn Host, dim: &str) {
    let waiting: Vec<u128> = {
        let waves = player_waves().lock().map(|m| m.keys().copied().collect::<Vec<_>>()).unwrap_or_default();
        let wear = hammer_state().lock().map(|s| s.1.iter().map(|(u, _)| *u).collect::<Vec<_>>()).unwrap_or_default();
        waves.into_iter().chain(wear).collect()
    };
    if waiting.is_empty() {
        return;
    }
    for player in host.server_players() {
        if player.dimension == dim && waiting.contains(&player.uuid) {
            hammer_tick(host, &player);
        }
    }
}

/// `PlayerEventsHandler.onEndPlayerTick`: one of the player's waves a tick
/// (6 to 8 to every other living thing on it), and the hammer's wear. Run
/// from the player's level tick, where that level is in place.
fn hammer_tick(host: &mut dyn Host, player: &NativeServerPlayer) {
    let wave = player_waves().lock().ok().and_then(|mut m| {
        let list = m.get_mut(&player.uuid)?;
        let w = list.pop_front();
        if list.is_empty() {
            m.remove(&player.uuid);
        }
        w
    });
    if let Some(wave) = wave {
        let dim = player.dimension.clone();
        // `mayBuild`: not in adventure mode.
        affect_blocks(host, &dim, &wave, player.game_mode != 2);
        let mut rng = Rng::new(player.uuid as u64 ^ now() ^ 0x55);
        let (min, max) = logic::wave_box(&wave, 0.0);
        let players = host.server_players();
        let attacker = (player.entity_id > 0).then_some(player.entity_id as u32);
        for p in players.iter().filter(|p| p.dimension == dim && p.uuid != player.uuid && p.entity_id > 0) {
            if player_in(p, min, max) {
                let amount = (6 + rng.int(3)) as f32;
                let _ = host.hurt_entity(&dim, p.entity_id as u32, amount, "mutant_monsters:player_seismic_wave", attacker);
            }
        }
        for id in host.entities_in(&dim, min, max) {
            if players.iter().any(|p| p.entity_id == id as i32) {
                continue;
            }
            let living = host.entity(id).and_then(|e| host.living(e)).is_some();
            if living {
                let amount = (6 + rng.int(3)) as f32;
                let _ = host.hurt_entity(&dim, id, amount, "mutant_monsters:player_seismic_wave", attacker);
            }
        }
    }
    // The wear of the uses this player made (`hurtAndBreak(1)`).
    let uses: Vec<usize> = hammer_state()
        .lock()
        .map(|mut s| {
            let mut mine = Vec::new();
            s.1.retain(|(uuid, slot)| {
                if *uuid == player.uuid {
                    mine.push(*slot);
                    false
                } else {
                    true
                }
            });
            mine
        })
        .unwrap_or_default();
    for slot in uses {
        wear_hammer(host, player, slot);
    }
}

fn wear_hammer(host: &mut dyn Host, player: &NativeServerPlayer, slot: usize) {
    if player.game_mode == 1 {
        return;
    }
    let Some(stacks) = host.player_stacks(player.uuid) else { return };
    let Some(Some(old)) = stacks.get(slot).cloned() else { return };
    if &*old.item != logic::HULK_HAMMER {
        return;
    }
    let mut new = old.clone();
    new.damage += 1;
    let broken = new.damage >= 150;
    let replacement = if broken { None } else { Some(new) };
    let ok = host
        .set_inventory_slot(player.uuid, NativeInventoryKind::Player, slot, Some(&old), replacement)
        .is_ok();
    if ok && broken {
        let _ = host.play_sound("minecraft:entity.item.break", player.position, 7, 0.8, 1.0);
    }
}

// --- Chemical X ------------------------------------------------------------------------

/// A mob a Chemical X splash is turning (`SkullSpirit`): the spirit flies
/// to it for `travel` ticks, then clings to it for `attached`.
#[derive(Clone, Debug)]
struct Conversion {
    dim: String,
    mob: u32,
    travel: i32,
    attached: i32,
    started: u64,
}

fn conversions() -> &'static Mutex<Vec<Conversion>> {
    static C: OnceLock<Mutex<Vec<Conversion>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(Vec::new()))
}

/// What Chemical X turns a mob into (`mutantXConversions`): the zombie and
/// the creeper have their mutant in this port so far.
fn mutant_of(kind: &str) -> Option<&'static str> {
    match logic::kind_id(kind) {
        "minecraft:zombie" => Some(logic::MUTANT_ZOMBIE),
        "minecraft:creeper" => Some(creeper::MUTANT_CREEPER),
        "minecraft:skeleton" => Some(skeleton::MUTANT_SKELETON),
        _ => None,
    }
}

/// The Chemical X bottle breaks: every grown mob within the splash
/// (4 x 2 x 4, as a splash potion's) that has a mutant gets a spirit.
pub fn projectile_impact(host: &mut dyn Host, event: &mut ProjectileImpactEvent<'_>) -> Flow {
    if logic::kind_id(event.kind) != logic::CHEMICAL_X {
        return Flow::Continue;
    }
    let at = event.at;
    let _ = host.play_sound("minecraft:entity.splash_potion.break", at, NEUTRAL, 1.0, 1.0);
    let mut fx = ParticleOptions::simple(ParticleType::Effect);
    fx.color = [0.1, 0.1, 0.1, 1.0];
    let _ = host.send_particles(&fx, None, at, [1.0, 0.5, 1.0], 0.1, 40);
    let min = [at[0] - 4.0, at[1] - 2.0, at[2] - 4.0];
    let max = [at[0] + 4.0, at[1] + 2.0, at[2] + 4.0];
    let dim = event.dimension.to_owned();
    let mut rng = Rng::new(u64::from(event.projectile) ^ now());
    for id in host.entities_in(&dim, min, max) {
        let Some(e) = host.entity(id) else { continue };
        let Some(kind) = host.entity_kind(e) else { continue };
        if mutant_of(&kind).is_none() {
            continue;
        }
        let baby = host
            .entity_nbt(e)
            .and_then(|n| n.get(&"IsBaby".into()).cloned())
            .is_some_and(|t| matches!(t, Tag::Byte(1)));
        if baby {
            continue;
        }
        let Ok(mut list) = conversions().lock() else { continue };
        if list.iter().any(|c| c.mob == id && c.dim == dim) {
            continue;
        }
        trace!("chemical x splash reaches {kind} {id}");
        list.push(Conversion { dim: dim.clone(), mob: id, travel: 15, attached: 80 + rng.int(40) as i32, started: now() });
    }
    Flow::Continue
}

/// The spirits of one level, each tick (`SkullSpirit.tick`).
pub fn level_tick_post(host: &mut dyn Host, event: &mut LevelTickPostEvent<'_>) {
    let dim = event.dimension;
    hammer_players(host, dim);
    let mut list = match conversions().lock() {
        Ok(mut l) => {
            if !l.iter().any(|c| c.dim == dim) {
                return;
            }
            std::mem::take(&mut *l)
        }
        Err(_) => return,
    };
    let mut rng = Rng::new(now() ^ 0xC0FFEE);
    list.retain_mut(|c| {
        if c.dim != dim {
            return true;
        }
        let Some(e) = host.entity(c.mob) else { return false };
        let Some(pos) = host.entity_position(e) else { return false };
        let soul = ParticleOptions::simple(ParticleType::Soul);
        if c.travel > 0 {
            // The spirit rises from the splash and homes in on the mob.
            c.travel -= 1;
            let at = [pos[0], pos[1] + 2.0 + f64::from(c.travel) * 0.15, pos[2]];
            let _ = host.send_particles(&soul, None, at, [0.1, 0.1, 0.1], 0.01, 2);
            return true;
        }
        c.attached -= 1;
        let _ = host.send_particles(&soul, None, [pos[0], pos[1] + 1.0, pos[2]], [0.4, 0.6, 0.4], 0.02, 3);
        let shake = [(rng.float() - rng.float()) as f64 * 0.1, 0.0, (rng.float() - rng.float()) as f64 * 0.1];
        let _ = host.push_mob(dim, c.mob, shake);
        if c.attached > 0 {
            return true;
        }
        let boom = ParticleOptions::simple(ParticleType::Explosion);
        let _ = host.send_particles(&boom, None, [pos[0], pos[1] + 1.0, pos[2]], [0.5, 0.5, 0.5], 0.0, 3);
        let _ = host.play_sound("minecraft:entity.generic.explode", pos, HOSTILE, 1.0, 1.0);
        let kind = host.entity_kind(e).unwrap_or_default();
        // One time in four the spirit fizzles out and the mob stays.
        if let Some(mutant) = mutant_of(&kind).filter(|_| rng.int(4) != 0) {
            convert(host, dim, c.mob, pos, mutant);
        }
        false
    });
    if let Ok(mut l) = conversions().lock() {
        list.append(&mut l);
        *l = list;
    }
}

/// `target.convertTo(mutant)`, then the blocks in the mutant's way broken
/// with their drops.
fn convert(host: &mut dyn Host, dim: &str, mob: u32, pos: [f64; 3], mutant: &str) {
    let mut nbt = Compound::new();
    nbt.insert("PersistenceRequired".into(), Tag::Byte(1));
    if let Some([yaw, _]) = host.entity(mob).and_then(|e| host.entity_rotation(e)) {
        nbt.insert("Rotation".into(), Tag::List(vec![Tag::Float(yaw.to_bits()), Tag::Float(0f32.to_bits())]));
    }
    if !host.remove_mob(dim, mob) {
        return;
    }
    let half = logic::WIDTH / 2.0;
    let (x0, x1) = ((pos[0] - half).floor() as i32, (pos[0] + half).floor() as i32);
    let (z0, z1) = ((pos[2] - half).floor() as i32, (pos[2] + half).floor() as i32);
    let (y0, y1) = (pos[1].floor() as i32, (pos[1] + logic::HEIGHT).floor() as i32);
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                let Some(state) = block_at(host, dim, [x, y, z]) else { continue };
                if matches!(logic::classify(&state), Cell::Air | Cell::Fluid { .. })
                    || unbreakable(logic::block_name(&state))
                {
                    continue;
                }
                let _ = host.destroy_block([x, y, z], None, false);
            }
        }
    }
    let summoned = host.summon(dim, mutant, pos, nbt);
    trace!("mob {mob} turns into {mutant}: {summoned:?}");
}

/// Blocks with a hardness of -1, which `destroyBlock` leaves alone upstream.
fn unbreakable(name: &str) -> bool {
    matches!(
        name,
        "minecraft:bedrock" | "minecraft:barrier" | "minecraft:end_portal" | "minecraft:end_portal_frame"
            | "minecraft:end_gateway" | "minecraft:nether_portal" | "minecraft:command_block"
            | "minecraft:chain_command_block" | "minecraft:repeating_command_block"
            | "minecraft:structure_block" | "minecraft:jigsaw" | "minecraft:light"
            | "minecraft:reinforced_deepslate" | "minecraft:moving_piston" | "minecraft:test_block"
            | "minecraft:test_instance_block"
    )
}
