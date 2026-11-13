//! The Mutant Zombie's pose: `MutantZombieModel.setupAnim` and its
//! `animateMelee`, `animateRoar`, `animateThrow` and `animateDeath`,
//! translated from Mutant Monsters (AGPL-3.0, see NOTICE.md) onto a plain
//! struct so it runs, and is tested, without the game. `client.rs` copies the
//! result onto the baked `ModelPart` tree.
use crate::logic::Anim;
use std::f32::consts::PI;

/// One part's place and turn, as `ModelPart.x/y/z` and `xRot/yRot/zRot`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Part {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub xr: f32,
    pub yr: f32,
    pub zr: f32,
}

impl Part {
    const fn new(x: f32, y: f32, z: f32, xr: f32, yr: f32, zr: f32) -> Self {
        Part { x, y, z, xr, yr, zr }
    }

    /// `Animator.addRotationAngle`.
    fn add(&mut self, x: f32, y: f32, z: f32) {
        self.xr += x;
        self.yr += y;
        self.zr += z;
    }
}

/// The parts `setupAnim` moves, by the path from the model's root, and their
/// initial pose from `createBodyLayer` (`model.rs` builds the mesh from these
/// same values).
pub const PELVIS: Part = Part::new(0.0, 10.0, 6.0, 0.0, 0.0, 0.0);
pub const WAIST: Part = Part::new(0.0, 0.0, 0.0, PI / 16.0, 0.0, 0.0);
pub const CHEST: Part = Part::new(0.0, -12.0, 0.0, PI / 6.0, 0.0, 0.0);
pub const HEAD: Part = Part::new(0.0, -11.0, -4.0, -11.0 * PI / 48.0, 0.0, 0.0);
pub const RIGHT_ARM: Part = Part::new(-11.0, -8.0, 2.0, -5.0 * PI / 48.0, 0.0, PI / 8.0);
pub const LEFT_ARM: Part = Part::new(11.0, -8.0, 2.0, -5.0 * PI / 48.0, 0.0, -PI / 8.0);
pub const ARM_LOWER: Part = Part::new(0.0, 14.0, 0.0, -PI / 3.0, 0.0, 0.0);
pub const RIGHT_LEG: Part = Part::new(-5.0, -2.0, 0.0, -PI / 4.0, 0.0, 0.0);
pub const LEFT_LEG: Part = Part::new(5.0, -2.0, 0.0, -PI / 4.0, 0.0, 0.0);
pub const LEG_LOWER: Part = Part::new(0.0, 9.5, 0.0, PI / 4.0, 0.0, 0.0);

pub const PATH_PELVIS: &[&str] = &["pelvis"];
pub const PATH_WAIST: &[&str] = &["pelvis", "waist"];
pub const PATH_CHEST: &[&str] = &["pelvis", "waist", "chest"];
pub const PATH_HEAD: &[&str] = &["pelvis", "waist", "chest", "head"];
pub const PATH_RIGHT_ARM: &[&str] = &["pelvis", "waist", "chest", "right_arm"];
pub const PATH_LEFT_ARM: &[&str] = &["pelvis", "waist", "chest", "left_arm"];
pub const PATH_RIGHT_ARM_LOWER: &[&str] = &["pelvis", "waist", "chest", "right_arm", "right_arm_lower"];
pub const PATH_LEFT_ARM_LOWER: &[&str] = &["pelvis", "waist", "chest", "left_arm", "left_arm_lower"];
pub const PATH_RIGHT_LEG: &[&str] = &["pelvis", "right_leg"];
pub const PATH_LEFT_LEG: &[&str] = &["pelvis", "left_leg"];

/// The moving parts of the model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rig {
    pub pelvis: Part,
    pub waist: Part,
    pub chest: Part,
    pub head: Part,
    pub right_arm: Part,
    pub left_arm: Part,
    pub right_arm_lower: Part,
    pub left_arm_lower: Part,
    pub right_leg: Part,
    pub left_leg: Part,
}

impl Default for Rig {
    /// `resetPose`: every part at its initial pose.
    fn default() -> Self {
        Rig {
            pelvis: PELVIS,
            waist: WAIST,
            chest: CHEST,
            head: HEAD,
            right_arm: RIGHT_ARM,
            left_arm: LEFT_ARM,
            right_arm_lower: ARM_LOWER,
            left_arm_lower: ARM_LOWER,
            right_leg: RIGHT_LEG,
            left_leg: LEFT_LEG,
        }
    }
}

impl Rig {
    /// Each part with the path that finds it in the baked tree.
    pub fn parts(&self) -> [(&'static [&'static str], Part); 10] {
        [
            (PATH_PELVIS, self.pelvis),
            (PATH_WAIST, self.waist),
            (PATH_CHEST, self.chest),
            (PATH_HEAD, self.head),
            (PATH_RIGHT_ARM, self.right_arm),
            (PATH_LEFT_ARM, self.left_arm),
            (PATH_RIGHT_ARM_LOWER, self.right_arm_lower),
            (PATH_LEFT_ARM_LOWER, self.left_arm_lower),
            (PATH_RIGHT_LEG, self.right_leg),
            (PATH_LEFT_LEG, self.left_leg),
        ]
    }
}

/// `MutantZombieRenderState`: what one frame's pose depends on.
#[derive(Clone, Copy, Debug, Default)]
pub struct PoseInput {
    pub walk_position: f32,
    pub walk_speed: f32,
    pub age: f32,
    /// The head's turn from the body and its pitch, in degrees.
    pub head_yaw: f32,
    pub pitch: f32,
    pub anim: Anim,
    /// Ticks into `anim`, with the partial tick.
    pub anim_time: f32,
    /// Ticks since it fell (`getDeathTime(partialTick)`), 0 when standing.
    pub death_time: f32,
    /// Ticks since the throw hit and since it landed, -1 before.
    pub throw_hit_time: f32,
    pub throw_finish_time: f32,
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// `setupAnim`.
pub fn setup_anim(s: &PoseInput) -> Rig {
    let mut r = Rig::default();
    let pos = s.walk_position;
    let speed = s.walk_speed;
    let mut right_leg_walk = (((pos - 0.7) * 0.4).sin() + 0.7) * speed;
    let mut left_leg_walk = -(((pos + 0.7) * 0.4).sin() - 0.7) * speed;
    let mut walk = (pos * 0.4).sin() * speed;
    let mut breathe = (s.age * 0.1).sin();
    let mut face_yaw = s.head_yaw * PI / 180.0;
    let mut face_pitch = s.pitch * PI / 180.0;
    if s.death_time <= 0.0 {
        if s.anim == Anim::Slam {
            animate_melee(&mut r, s.anim_time);
        }
        if s.anim == Anim::Roar {
            animate_roar(&mut r, s.anim_time);
            let scale = 1.0 - clamp01(s.anim_time / 6.0);
            right_leg_walk *= scale;
            left_leg_walk *= scale;
            walk *= scale;
            face_pitch *= scale;
        }
        if s.anim == Anim::Throw {
            animate_throw(&mut r, s);
            let scale = 1.0 - clamp01(s.anim_time / 3.0);
            right_leg_walk *= scale;
            left_leg_walk *= scale;
            walk *= scale;
            face_pitch *= scale;
        }
    } else {
        animate_death(&mut r, s.death_time);
        let scale = 1.0 - clamp01(s.death_time / 6.0);
        right_leg_walk *= scale;
        left_leg_walk *= scale;
        walk *= scale;
        breathe *= scale;
        face_yaw *= scale;
        face_pitch *= scale;
    }
    r.chest.xr += breathe * 0.02;
    r.right_arm.zr -= breathe * 0.05;
    r.left_arm.zr += breathe * 0.05;
    r.head.xr += face_pitch * 0.6;
    r.head.yr += face_yaw * 0.8;
    r.head.zr -= face_yaw * 0.2;
    r.chest.xr += face_pitch * 0.4;
    r.chest.yr += face_yaw * 0.2;
    r.pelvis.y += (pos * 0.8).sin() * speed * 0.5;
    r.chest.yr -= walk * 0.1;
    r.right_arm.xr -= walk * 0.6;
    r.left_arm.xr += walk * 0.6;
    r.right_leg.xr += right_leg_walk * 0.9;
    r.left_leg.xr += left_leg_walk * 0.9;
    r
}

fn animate_melee(r: &mut Rig, t: f32) {
    r.right_arm.zr = 0.0;
    r.left_arm.zr = 0.0;
    if t < 8.0 {
        let progress = t / 8.0;
        let swing = -(progress * PI / 2.0).sin();
        let sway = (progress * PI / 2.0).cos();
        r.waist.xr += swing * 0.2;
        r.chest.xr += swing * 0.2;
        r.right_arm.xr += swing * 2.3;
        r.right_arm.zr += sway * PI / 8.0;
        r.left_arm.xr += swing * 2.3;
        r.left_arm.zr -= sway * PI / 8.0;
        r.right_arm_lower.xr += swing * 0.8;
        r.left_arm_lower.xr += swing * 0.8;
    } else if t < 12.0 {
        let progress = (t - 8.0) / 4.0;
        let swing = -(progress * PI / 2.0).cos();
        let sway = (progress * PI / 2.0).sin();
        r.waist.xr += swing * 0.9 + 0.7;
        r.chest.xr += swing * 0.9 + 0.7;
        r.right_arm.xr += swing * 0.2 - 2.1;
        r.right_arm.zr += sway * 0.3;
        r.left_arm.xr += swing * 0.2 - 2.1;
        r.left_arm.zr -= sway * 0.3;
        r.right_arm_lower.xr += swing + 0.2;
        r.left_arm_lower.xr += swing + 0.2;
    } else if t < 16.0 {
        r.waist.xr += 0.7;
        r.chest.xr += 0.7;
        r.right_arm.xr -= 2.1;
        r.right_arm.zr += 0.3;
        r.left_arm.xr -= 2.1;
        r.left_arm.zr -= 0.3;
        r.right_arm_lower.xr += 0.2;
        r.left_arm_lower.xr += 0.2;
    } else if t < 24.0 {
        let progress = (t - 16.0) / 8.0;
        let swing = (progress * PI / 2.0).cos();
        r.waist.xr += swing * 0.7;
        r.chest.xr += swing * 0.7;
        r.right_arm.xr -= swing * 2.1;
        r.right_arm.zr += swing * -0.092_699_08 + PI / 8.0;
        r.left_arm.xr -= swing * 2.1;
        r.left_arm.zr -= swing * -0.092_699_08 + PI / 8.0;
        r.right_arm_lower.xr += swing * 0.2;
        r.left_arm_lower.xr += swing * 0.2;
    } else {
        r.right_arm.zr += PI / 8.0;
        r.left_arm.zr += -PI / 8.0;
    }
}

fn animate_roar(r: &mut Rig, t: f32) {
    if t < 10.0 {
        let progress = t / 10.0;
        let roar = (progress * PI / 2.0).sin();
        let sway = (progress * PI * PI / 8.0).sin();
        r.waist.xr += roar * 0.2;
        r.chest.xr += roar * 0.4;
        r.chest.yr += sway * 0.06;
        r.head.xr += roar * 0.8;
        r.right_arm.xr -= roar * 1.2;
        r.right_arm.zr += roar * 0.6;
        r.left_arm.xr -= roar * 1.2;
        r.left_arm.zr -= roar * 0.6;
        r.right_arm_lower.xr -= roar * 0.8;
        r.left_arm_lower.xr -= roar * 0.8;
    } else if t < 15.0 {
        let progress = (t - 10.0) / 5.0;
        let roar = (progress * PI / 2.0).cos();
        let sway = (progress * PI / 2.0).sin();
        r.waist.xr += roar * 0.396_349_55 - PI / 16.0;
        r.chest.xr += roar * 0.6 - 0.2;
        r.head.xr += roar - 0.2;
        r.right_arm.xr -= roar * 2.2 - 1.0;
        r.right_arm.yr += sway * 0.4;
        r.right_arm.zr += 0.6;
        r.left_arm.xr -= roar * 2.2 - 1.0;
        r.left_arm.yr -= sway * 0.4;
        r.left_arm.zr -= 0.6;
        r.right_arm_lower.xr -= roar - 0.2;
        r.left_arm_lower.xr -= roar - 0.2;
        r.right_leg.yr += sway * 0.3;
        r.left_leg.yr -= sway * 0.3;
    } else if t < 75.0 {
        r.waist.xr -= PI / 16.0;
        r.chest.xr -= 0.2;
        r.head.xr -= 0.2;
        r.right_arm.add(1.0, 0.4, 0.6);
        r.left_arm.add(1.0, -0.4, -0.6);
        r.right_arm_lower.xr += 0.2;
        r.left_arm_lower.xr += 0.2;
        r.right_leg.yr += 0.3;
        r.left_leg.yr -= 0.3;
    } else if t < 90.0 {
        let progress = (t - 75.0) / 15.0;
        let roar = (progress * PI / 2.0).cos();
        r.waist.xr -= roar * 0.696_349_56 - 0.5;
        r.chest.xr -= roar * 0.7 - 0.5;
        r.head.xr -= roar * 0.6 - 0.4;
        r.right_arm.add(roar * 2.6 - 1.6, roar * 0.4, roar * 0.992_699_15 - PI / 8.0);
        r.left_arm.add(roar * 2.6 - 1.6, -roar * 0.4, -roar * 0.992_699_15 + PI / 8.0);
        r.right_arm_lower.xr += roar * -0.6 + 0.8;
        r.left_arm_lower.xr += roar * -0.6 + 0.8;
        r.right_leg.yr += roar * 0.3;
        r.left_leg.yr -= roar * 0.3;
    } else if t < 110.0 {
        r.waist.xr += 0.5;
        r.chest.xr += 0.5;
        r.head.xr += 0.4;
        r.right_arm.add(-1.6, 0.0, -PI / 8.0);
        r.left_arm.add(-1.6, 0.0, PI / 8.0);
        r.right_arm_lower.xr += 0.8;
        r.left_arm_lower.xr += 0.8;
    } else {
        let progress = (t - 110.0) / 10.0;
        let roar = (progress * PI / 2.0).cos();
        r.waist.xr += roar * 0.5;
        r.chest.xr += roar * 0.5;
        r.head.xr += roar * 0.4;
        r.right_arm.add(roar * -1.6, 0.0, roar * -PI / 8.0);
        r.left_arm.add(roar * -1.6, 0.0, roar * PI / 8.0);
        r.right_arm_lower.xr += roar * 0.8;
        r.left_arm_lower.xr += roar * 0.8;
    }
    if (10.0..75.0).contains(&t) {
        let progress = (t - 10.0) / 65.0;
        let roar = (progress * PI * 8.0).sin();
        let sway = (progress * PI * 8.0 + PI / 4.0).sin();
        r.head.yr += roar * 0.5 - sway * 0.2;
        r.head.zr -= roar * 0.5;
        r.chest.yr += sway * 0.06;
    }
}

fn animate_throw(r: &mut Rig, s: &PoseInput) {
    let t = s.anim_time;
    if t < 3.0 {
        let progress = t / 3.0;
        let throw = (progress * PI / 2.0).sin();
        r.chest.xr -= throw * 0.4;
        r.right_arm.xr -= throw * 1.8;
        r.right_arm.zr -= throw * PI / 8.0;
        r.left_arm.xr -= throw * 1.8;
        r.left_arm.zr += throw * PI / 8.0;
    } else if t < 5.0 {
        r.chest.xr -= 0.4;
        r.right_arm.xr -= 1.8;
        r.right_arm.zr = 0.0;
        r.left_arm.xr -= 1.8;
        r.left_arm.zr = 0.0;
    } else if t < 8.0 {
        let progress = (t - 5.0) / 3.0;
        let throw = (progress * PI / 2.0).cos();
        let lift = (progress * PI / 2.0).sin();
        r.waist.xr += lift * 0.2;
        r.chest.xr -= throw * 0.6 - 0.2;
        r.right_arm.xr -= throw * 2.2 - 0.4;
        r.right_arm.zr -= throw * PI / 8.0;
        r.left_arm.xr -= throw * 2.2 - 0.4;
        r.left_arm.zr += throw * PI / 8.0;
        r.right_arm_lower.xr -= lift * 0.4;
        r.left_arm_lower.xr -= lift * 0.4;
    } else if t < 10.0 {
        r.waist.xr += 0.2;
        r.chest.xr += 0.2;
        r.right_arm.xr += 0.4;
        r.left_arm.xr += 0.4;
        r.right_arm_lower.xr -= 0.4;
        r.left_arm_lower.xr -= 0.4;
    } else if t < 15.0 {
        let progress = (t - 10.0) / 5.0;
        let throw = (progress * PI / 2.0).cos();
        let lift = (progress * PI / 2.0).sin();
        r.waist.xr += throw * 0.396_349_55 - PI / 16.0;
        r.chest.xr += throw * 0.8 - 0.6;
        r.right_arm.xr += throw * 3.0 - 2.6;
        r.left_arm.xr += throw * 3.0 - 2.6;
        r.right_arm_lower.xr -= throw * 0.4;
        r.left_arm_lower.xr -= throw * 0.4;
        r.right_leg.xr += lift * 0.6;
        r.left_leg.xr += lift * 0.6;
    } else if s.throw_hit_time < 0.0 {
        r.waist.xr -= PI / 16.0;
        r.chest.xr -= 0.6;
        r.right_arm.xr -= 2.6;
        r.left_arm.xr -= 2.6;
        r.right_leg.xr += 0.6;
        r.left_leg.xr += 0.6;
    } else if s.throw_hit_time < 5.0 {
        let progress = s.throw_hit_time / 3.0;
        let throw = (progress * PI / 2.0).cos();
        let lift = (progress * PI / 2.0).sin();
        r.waist.xr -= throw * 0.396_349_55 - 0.2;
        r.chest.xr -= throw * 0.8 - 0.2;
        r.right_arm.add(-(throw * 2.2 + 0.4), -lift * PI / 8.0, lift * 0.4);
        r.left_arm.add(-(throw * 2.2 + 0.4), lift * PI / 8.0, -lift * 0.4);
        r.right_arm_lower.xr += lift * 0.2;
        r.left_arm_lower.xr += lift * 0.2;
        r.right_leg.xr += throw * 0.8 - 0.2;
        r.left_leg.xr += throw * 0.8 - 0.2;
    } else if s.throw_finish_time < 0.0 {
        r.waist.xr += 0.2;
        r.chest.xr += 0.2;
        r.right_arm.add(-0.4, -PI / 8.0, 0.4);
        r.left_arm.add(-0.4, PI / 8.0, -0.4);
        r.right_arm_lower.xr += 0.2;
        r.left_arm_lower.xr += 0.2;
        r.right_leg.xr -= 0.2;
        r.left_leg.xr -= 0.2;
    } else if s.throw_finish_time < 10.0 {
        let progress = s.throw_finish_time / 10.0;
        let throw = (progress * PI / 2.0).cos();
        r.waist.xr += throw * 0.2;
        r.chest.xr += throw * 0.2;
        // Upstream turns the right arm twice here and the left arm not at
        // all; kept as it is, so the recovery looks the same.
        r.right_arm.add(-throw * 0.4, -throw * PI / 8.0, throw * 0.4);
        r.right_arm.add(-throw * 0.4, throw * PI / 8.0, -throw * 0.4);
        r.right_arm_lower.xr += throw * 0.2;
        r.left_arm_lower.xr += throw * 0.2;
        r.right_leg.xr -= throw * 0.2;
        r.left_leg.xr -= throw * 0.2;
    }
}

/// The fallen pose at `amount` (0 standing, 1 flat out).
fn fallen(r: &mut Rig, amount: f32, arm_yaw: f32) {
    r.pelvis.y += amount * 28.0;
    r.head.xr -= amount * PI / 10.0;
    r.head.yr += amount * PI / 5.0;
    r.chest.xr -= amount * PI / 12.0;
    r.waist.xr -= amount * PI / 10.0;
    r.right_arm.xr -= amount * PI / 2.0;
    r.right_arm.yr += arm_yaw;
    r.left_arm.xr -= amount * PI / 2.0;
    r.left_arm.yr -= arm_yaw;
    r.right_leg.xr += amount * PI / 6.0;
    r.right_leg.zr += amount * PI / 12.0;
    r.left_leg.xr += amount * PI / 6.0;
    r.left_leg.zr -= amount * PI / 12.0;
}

fn animate_death(r: &mut Rig, death: f32) {
    if death <= 20.0 {
        let progress = (death - 1.0) / 20.0;
        let amount = (progress * PI / 2.0).sin();
        fallen(r, amount, amount * PI / 2.8);
    } else if death <= 100.0 {
        fallen(r, 1.0, 1.121_997_5);
    } else {
        let progress = (40.0 - (140.0 - death)) / 40.0;
        let amount = (progress * PI / 2.0).cos();
        fallen(r, amount, amount * PI / 2.8);
    }
}

/// `MutantZombieRenderer.setupRotations` while down: how far the body tips
/// over backwards, in degrees (`getFlipDegrees` is 80).
pub fn death_flip_degrees(death: f32) -> f32 {
    if death <= 0.0 {
        return 0.0;
    }
    let (amount, reviving) = if death > 100.0 {
        (140.0 - death, true)
    } else {
        (death.min(20.0), false)
    };
    if amount <= 0.0 {
        return 0.0;
    }
    let flip = if reviving {
        amount / 40.0 * 1.6
    } else {
        (amount - 1.0) / 20.0 * 1.6
    };
    flip.max(0.0).sqrt().min(1.0) * 80.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_still_is_the_rest_pose_but_for_breathing() {
        let rig = setup_anim(&PoseInput { throw_hit_time: -1.0, throw_finish_time: -1.0, ..Default::default() });
        assert_eq!(rig.waist, WAIST);
        assert_eq!(rig.head, HEAD);
        assert_eq!(rig.pelvis, PELVIS);
    }

    #[test]
    fn slam_raises_the_arms_then_brings_them_down() {
        let at = |t: f32| {
            setup_anim(&PoseInput { anim: Anim::Slam, anim_time: t, throw_hit_time: -1.0, throw_finish_time: -1.0, ..Default::default() })
        };
        // Arms go back over the head (xRot well below the rest pose)...
        assert!(at(7.9).right_arm.xr < RIGHT_ARM.xr - 2.0);
        // ...and the body bends forward on the hit.
        assert!(at(13.0).waist.xr > WAIST.xr + 0.6);
        let end = at(30.0);
        assert!((end.right_arm.zr - PI / 8.0).abs() < 1e-6);
    }

    #[test]
    fn down_and_up_again() {
        let down = setup_anim(&PoseInput { death_time: 60.0, ..Default::default() });
        assert!((down.pelvis.y - (PELVIS.y + 28.0)).abs() < 1e-4);
        let up = setup_anim(&PoseInput { death_time: 140.0, ..Default::default() });
        assert!((up.pelvis.y - PELVIS.y).abs() < 1e-3);
        assert_eq!(death_flip_degrees(0.0), 0.0);
        assert!((death_flip_degrees(60.0) - 80.0).abs() < 1e-4);
        assert!(death_flip_degrees(139.0) < 20.0);
    }

    #[test]
    fn roar_and_throw_stop_the_legs() {
        let walking = PoseInput { walk_position: 3.0, walk_speed: 1.0, throw_hit_time: -1.0, throw_finish_time: -1.0, ..Default::default() };
        let roar = setup_anim(&PoseInput { anim: Anim::Roar, anim_time: 30.0, ..walking });
        assert!((roar.right_leg.xr - (RIGHT_LEG.xr)).abs() < 1e-6);
        let throw = setup_anim(&PoseInput { anim: Anim::Throw, anim_time: 20.0, ..walking });
        assert!((throw.right_leg.xr - (RIGHT_LEG.xr + 0.6)).abs() < 1e-6);
    }
}
