//! The Mutant Zombie's goals (`MutantZombie.registerGoals`), translated from
//! Mutant Monsters (AGPL-3.0, see NOTICE.md) onto Horizon's `Goal` trait
//! (docs/native-mods.md, "Mob goals").
//!
//! Upstream's three `AnimationGoal`s (slam, roar, throw) act on the world
//! (waves, pushes, zombies from the ground), which a goal can't reach in
//! Horizon: here a goal only *starts* an attack, by writing it to the brain
//! (`brain.rs`), and `Busy` holds the zombie still while `server.rs` plays it
//! out with the host's helpers.
use crate::brain::{self, TargetSnap};
use crate::logic::{self, Anim, Rng};
use horizon_client::mobs::{
    Mob, Species, Who,
    goals::{Cx, Goal, HurtByTarget, LOOK, LookAtPlayer, MOVE, NearestTarget, RandomLookAround, Stroll},
};
use std::sync::Arc;

/// Give a new Mutant Zombie its goals, size and daylight immunity. Asked for
/// every new mob, on the server and on a client (for its copies).
pub fn register(mob: &mut Mob) {
    crate::creeper::register(mob);
    crate::skeleton::register(mob);
    if mob.kind.id != logic::MUTANT_ZOMBIE {
        return;
    }
    // The entity type copies the zombie: its size (0.6 x 1.95) and its
    // burning in daylight. A Mutant Zombie is 1.8 x 3.2 and isn't a Zombie
    // (it extends Monster upstream), so it doesn't burn.
    let kind = Arc::make_mut(&mut mob.kind);
    kind.width = logic::WIDTH;
    kind.height = logic::HEIGHT;
    kind.eye_height = logic::EYE_HEIGHT;
    kind.flags.burns_in_day = false;

    let seed = u64::from(mob.id).wrapping_mul(0x9E37_79B9);
    let goals = &mut mob.ai.goals;
    goals.clear();
    goals.add(0, horizon_client::mobs::goals::Float);
    goals.add(0, Sense { rng: Rng::new(seed ^ 1) });
    goals.add(0, Busy::default());
    goals.add(1, MutantMelee::new(1.2, seed));
    goals.add(4, Stroll::avoiding_water(1.0));
    goals.add(5, LookAtPlayer::new(8.0));
    goals.add(6, RandomLookAround::default());

    let targets = &mut mob.ai.targets;
    targets.clear();
    // HurtByNearestTargetGoal: never at another zombie (`canHarm`).
    targets.add(
        0,
        HurtByTarget::new().ignoring(&[
            Species::Zombie,
            Species::Husk,
            Species::Drowned,
            Species::ZombieVillager,
            Species::ZombifiedPiglin,
            Species::Zoglin,
        ]),
    );
    targets.add(1, NearestTarget::species(&[Species::IronGolem], true));
    targets.add(2, NearestTarget::player(true).unseen_memory(300));
    targets.add(3, NearestTarget::species(&[Species::Villager, Species::WanderingTrader], true));
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

/// Writes what the server half needs of the mob each tick (no flags: it runs
/// beside every other goal), and makes `customServerAiStep`'s slam and
/// `RoarGoal.canUse`'s roar decisions.
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
        let target = snapshot(m, cx);
        let pos = [m.position.x, m.position.y, m.position.z];
        let on_ground = m.on_ground;
        let in_fluid = m.fluids.in_water;
        let health = m.health;
        let age = m.age;
        let game_time = cx.w.game_time;
        let rng = &mut self.rng;
        brain::with(m.id, |b| {
            b.pos = pos;
            b.on_ground = on_ground;
            b.in_fluid = in_fluid;
            b.health = health;
            b.age = age;
            b.target = target.filter(|t| t.alive);
            b.seen = game_time;
            if b.busy() {
                return;
            }
            let Some(t) = b.target else { return };
            if logic::ai_step_slam(rng, pos, t.pos) {
                b.anim = Anim::Slam;
                b.anim_tick = 0;
            } else if logic::roar_choice(rng, age, on_ground, b.resurrecting, pos, t.pos) {
                b.anim = Anim::Roar;
                b.anim_tick = 0;
            }
        });
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

/// The three `AnimationGoal`s' hold on the zombie, and its stillness while it
/// is down: no walking and no other look while an attack plays.
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
        brain::get(m.id).is_some_and(|b| b.busy())
    }
    fn every_tick(&self) -> bool {
        true
    }
    fn start(&mut self, m: &mut Mob, _: &mut Cx) {
        m.nav_stop();
    }
    fn tick(&mut self, m: &mut Mob, cx: &mut Cx) {
        m.nav_stop();
        let Some(b) = brain::get(m.id) else { return };
        if b.down {
            return;
        }
        // SlamGroundGoal looks at the target until it swings (tick 8),
        // RoarGoal until tick 75, ThrowAttackGoal the whole time.
        let looks = match b.anim {
            Anim::Slam => b.anim_tick < 8,
            Anim::Roar => b.anim_tick < 75,
            Anim::Throw => true,
            _ => false,
        };
        if looks {
            if let Some(t) = snapshot(m, cx) {
                m.look_at_speed([t.pos[0], t.eye_y, t.pos[2]], 30.0, 30.0);
            }
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}

/// `MutantMeleeAttackGoal(this, 1.2).setMaxAttackTick(0)`: walk at the
/// target, and once within reach start what `MutantZombie.doHurtTarget`
/// starts, a slam or a throw.
#[derive(Clone, Debug)]
struct MutantMelee {
    speed: f64,
    recalc: i32,
    rng: Rng,
}

impl MutantMelee {
    fn new(speed: f64, seed: u64) -> Self {
        MutantMelee { speed, recalc: 0, rng: Rng::new(seed ^ 2) }
    }
}

impl Goal for MutantMelee {
    fn flags(&self) -> u8 {
        MOVE | LOOK
    }
    fn can_use(&mut self, m: &mut Mob, cx: &mut Cx) -> bool {
        if brain::get(m.id).is_some_and(|b| b.busy()) {
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
        // maxAttackTick 0: every tick in reach is a hit.
        if m.within_melee_range(cx.w, &t) && m.sees(cx.w, who) {
            let grounded = m.on_ground || m.fluids.in_water;
            let rng = &mut self.rng;
            brain::with(m.id, |b| {
                if !b.busy() {
                    if let Some(anim) = logic::melee_choice(rng, grounded) {
                        b.anim = anim;
                        b.anim_tick = 0;
                    }
                }
            });
        }
    }
    fn box_clone(&self) -> Box<dyn Goal> {
        Box::new(self.clone())
    }
}
