//! The client half: `MutantZombieRenderer` and `MutantZombieModel` over
//! Horizon's mob drawing (docs/native-mods.md, "Mobs drawn by a mod"), posed
//! from what the server says each Mutant Zombie is doing (`net.rs`), and the
//! pushes the server sends this player.
use crate::logic::{self, Anim};
use crate::model;
use crate::net::{self, AnimMessage, PushMessage};
use crate::pose::{self, PoseInput};
use horizon_client::mod_api::{Flow, Host, LayerDefinitionsEvent, MobSubmitEvent, ModelPart, NetFromServerEvent};
use std::{
    collections::HashMap,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU32, Ordering},
    },
};

/// The client's own tick count, which the animations are timed with.
static TICKS: AtomicU32 = AtomicU32::new(0);

/// What one zombie is playing, by its entity id.
#[derive(Clone, Copy, Debug)]
struct Playing {
    anim: Anim,
    /// The client tick the animation started on.
    start: u32,
    hit_at: Option<u32>,
    finish_at: Option<u32>,
}

fn playing() -> &'static Mutex<HashMap<u32, Playing>> {
    static P: OnceLock<Mutex<HashMap<u32, Playing>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The baked model, made once (`context.bakeLayer`).
fn baked() -> &'static Mutex<Option<ModelPart>> {
    static M: OnceLock<Mutex<Option<ModelPart>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(None))
}

pub fn tick() {
    let now = TICKS.fetch_add(1, Ordering::Relaxed) + 1;
    crate::creeper::client_tick(now);
    crate::skeleton::client_tick(now);
    // An animation the server never ended (the zombie left view) is dropped.
    if now % 100 == 0 {
        if let Ok(mut map) = playing().lock() {
            map.retain(|_, p| now.saturating_sub(p.start) < p.anim.duration() + 400);
        }
    }
}

pub fn layer_definitions(event: &mut LayerDefinitionsEvent<'_>) {
    event.layers.extend(model::layers());
}

pub fn net_from_server(host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
    if event.channel == net::PUSH {
        if let Some(PushMessage(v)) = PushMessage::decode(event.data) {
            host.push_player(v);
        }
        return;
    }
    if event.channel == crate::creeper::CHANNEL {
        crate::creeper::heard(event.data, TICKS.load(Ordering::Relaxed));
        return;
    }
    if event.channel == crate::skeleton::CHANNEL {
        crate::skeleton::heard(event.data, TICKS.load(Ordering::Relaxed));
        return;
    }
    if event.channel != net::ANIM {
        return;
    }
    let Some(msg) = AnimMessage::decode(event.data) else { return };
    let Some(anim) = Anim::from_id(msg.anim) else { return };
    let now = TICKS.load(Ordering::Relaxed);
    let Ok(mut map) = playing().lock() else { return };
    if anim == Anim::None {
        map.remove(&msg.entity);
        return;
    }
    let start = now.saturating_sub(u32::from(msg.elapsed));
    let entry = map.entry(msg.entity).or_insert(Playing { anim, start, hit_at: None, finish_at: None });
    // A new animation, or this one resent far off our clock: take the
    // server's timing.
    if entry.anim != anim || entry.start.abs_diff(start) > 3 {
        *entry = Playing { anim, start, hit_at: None, finish_at: None };
    }
    if msg.flags & net::THROW_HIT != 0 && entry.hit_at.is_none() {
        entry.hit_at = Some(now);
    }
    if msg.flags & net::THROW_FINISHED != 0 && entry.finish_at.is_none() {
        entry.finish_at = Some(now);
    }
}

fn part_mut<'a>(root: &'a mut ModelPart, path: &[&str]) -> Option<&'a mut ModelPart> {
    let mut at = root;
    for name in path {
        at = at.get_child_mut(name)?;
    }
    Some(at)
}

/// Draw a Mutant Zombie with its own model (`MutantZombieRenderer`).
/// `Flow::Cancel` leaves the copied zombie's model out.
pub fn mob_submit(host: &dyn Host, event: &mut MobSubmitEvent<'_>) -> Flow {
    let mob = event.mob;
    if logic::kind_id(mob.kind) == crate::creeper::MUTANT_CREEPER {
        let now = TICKS.load(Ordering::Relaxed) as f32 + mob.partial;
        return crate::creeper::submit(host, mob, &mut *event.draw, now);
    }
    if logic::kind_id(mob.kind) == crate::skeleton::MUTANT_SKELETON {
        let now = TICKS.load(Ordering::Relaxed) as f32 + mob.partial;
        return crate::skeleton::submit(host, mob, &mut *event.draw, now);
    }
    if logic::kind_id(mob.kind) != logic::MUTANT_ZOMBIE {
        return Flow::Continue;
    }
    let model = {
        let Ok(mut cache) = baked().lock() else { return Flow::Continue };
        if cache.is_none() {
            *cache = host.bake_layer(model::LAYER);
        }
        cache.clone()
    };
    // Not baked (no layer): the zombie's own model is drawn instead.
    let Some(mut model) = model else { return Flow::Continue };

    let now = TICKS.load(Ordering::Relaxed) as f32 + mob.partial;
    let since = |tick: u32| (now - tick as f32).max(0.0);
    let state = playing().lock().ok().and_then(|m| m.get(&mob.id).copied());
    let mut input = PoseInput {
        walk_position: mob.walk_position,
        walk_speed: mob.walk_speed,
        age: mob.age,
        head_yaw: mob.head_yaw,
        pitch: mob.pitch,
        throw_hit_time: -1.0,
        throw_finish_time: -1.0,
        ..Default::default()
    };
    match state {
        Some(p) if p.anim == Anim::Death => {
            input.death_time = since(p.start).clamp(0.01, logic::REVIVE_AFTER_DEATH_TIME as f32);
        }
        Some(p) => {
            input.anim = p.anim;
            input.anim_time = since(p.start);
            input.throw_hit_time = p.hit_at.map(since).unwrap_or(-1.0);
            input.throw_finish_time = p.finish_at.map(since).unwrap_or(-1.0);
        }
        None => {}
    }
    // Its last death: the game's own death time (it doesn't get up again).
    if mob.death_time > 0 {
        input.anim = Anim::None;
        input.death_time = (mob.death_time as f32 + mob.partial).min(20.0);
    }
    let rig = pose::setup_anim(&input);
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

    let draw = &mut *event.draw;
    draw.push();
    // `getRenderOffset`.
    draw.translate([0.0, -0.0975, 0.0]);
    // `setupRotations` while down: it tips forward onto its face.
    let flip = pose::death_flip_degrees(input.death_time);
    if flip > 0.0 {
        draw.rotate_about([1.0, 0.0, 0.0], flip, [0.0, 0.0, 0.0]);
    }
    // Into the model's frame (+y down, front towards -z), at 1.3 times size.
    draw.scale([1.0, -1.0, -1.0]);
    draw.scale([1.3, 1.3, 1.3]);
    draw.translate([0.0, -1.501, 0.0]);
    // `hasRedOverlay`: hurt, or dying for good.
    let tint = if mob.hurt_time > 0 || mob.death_time > 0 { 0xFF9999 } else { 0xFFFFFF };
    host.draw_model_part(draw, &model, model::TEXTURE, tint);
    draw.pop();
    Flow::Cancel
}
