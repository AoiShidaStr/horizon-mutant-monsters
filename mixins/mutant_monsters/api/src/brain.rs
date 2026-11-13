//! What the goals (`goals.rs`, inside the mob's AI step) and the server hooks
//! (`server.rs`, after the mob ticked) share about each Mutant Zombie, by its
//! entity id. Upstream keeps this on the entity (`animation`,
//! `animationTick`, `DATA_THROW_ATTACK_STATE`); a Horizon mod keeps it here.
//! Both sides run on the server thread, one after the other.
use crate::logic::Anim;
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

/// The target as the goals saw it this tick (`TargetInfo`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TargetSnap {
    /// The mob's id; `None` for a player (the server finds which one by
    /// `pos`).
    pub mob: Option<u32>,
    pub is_player: bool,
    pub pos: [f64; 3],
    pub eye_y: f64,
    pub alive: bool,
    pub creative: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Brain {
    // Written by the `Sense` goal each tick.
    pub pos: [f64; 3],
    pub on_ground: bool,
    pub in_fluid: bool,
    pub health: f32,
    pub age: u32,
    pub target: Option<TargetSnap>,
    /// The game tick the goals last wrote this (`MobWorld.game_time`).
    pub seen: i64,
    // Written by the goals to start an attack, by the server to run it.
    pub anim: Anim,
    pub anim_tick: u32,
    /// Lying down between two lives: the goals hold still.
    pub down: bool,
    /// Zombies are still climbing out of the ground from its last roar
    /// (`resurrectionList` not empty).
    pub resurrecting: bool,
}

impl Brain {
    pub fn busy(&self) -> bool {
        self.down || self.anim != Anim::None
    }
}

pub fn brains() -> &'static Mutex<HashMap<u32, Brain>> {
    static BRAINS: OnceLock<Mutex<HashMap<u32, Brain>>> = OnceLock::new();
    BRAINS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn with<R>(id: u32, f: impl FnOnce(&mut Brain) -> R) -> Option<R> {
    let mut map = brains().lock().ok()?;
    Some(f(map.entry(id).or_default()))
}

pub fn get(id: u32) -> Option<Brain> {
    brains().lock().ok()?.get(&id).cloned()
}

/// Downed zombies, by entity id: every hit on them is refused
/// (`living_incoming_damage` has no host to ask).
pub fn downed() -> &'static Mutex<HashSet<i32>> {
    static DOWN: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();
    DOWN.get_or_init(|| Mutex::new(HashSet::new()))
}

/// A zombie throwing its target can't be hurt by that target
/// (`MutantZombie.hurtServer`): zombie id -> target entity id.
pub fn throwing() -> &'static Mutex<HashMap<i32, i32>> {
    static THROWING: OnceLock<Mutex<HashMap<i32, i32>>> = OnceLock::new();
    THROWING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Downed zombies hit by fire: they lose their remaining lives.
pub fn ignited() -> &'static Mutex<HashSet<i32>> {
    static IGNITED: OnceLock<Mutex<HashSet<i32>>> = OnceLock::new();
    IGNITED.get_or_init(|| Mutex::new(HashSet::new()))
}
