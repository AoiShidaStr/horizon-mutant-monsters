//! The two channels between the server half and the client half
//! (`horizon:mod_net`, docs/native-mods.md, "Mod messages").
//!
//! - `mutant_monsters:anim` replaces upstream's `ClientboundAnimationMessage`
//!   and the throw flags of `DATA_THROW_ATTACK_STATE`: which animation a
//!   Mutant Zombie plays and how far into it it is.
//! - `mutant_monsters:push` replaces `EntityUtil.sendPlayerVelocityPacket`:
//!   Horizon's server can't set a player's motion, so the player's own client
//!   adds the push (`native_push_player`).

pub const ANIM: &str = "mutant_monsters:anim";
pub const PUSH: &str = "mutant_monsters:push";

/// The throw has hit its target (`hasThrowAttackHit`).
pub const THROW_HIT: u8 = 1;
/// The throw has landed (`isThrowAttackFinished`).
pub const THROW_FINISHED: u8 = 2;

/// `[entity u32][anim u8][elapsed u16][flags u8]`, little endian.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnimMessage {
    pub entity: u32,
    pub anim: u8,
    /// Ticks since the animation started, so a player who comes near in the
    /// middle of one sees the right pose.
    pub elapsed: u16,
    pub flags: u8,
}

impl AnimMessage {
    pub const LEN: usize = 8;

    pub fn encode(self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        b[0..4].copy_from_slice(&self.entity.to_le_bytes());
        b[4] = self.anim;
        b[5..7].copy_from_slice(&self.elapsed.to_le_bytes());
        b[7] = self.flags;
        b
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() != Self::LEN {
            return None;
        }
        Some(Self {
            entity: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            anim: data[4],
            elapsed: u16::from_le_bytes([data[5], data[6]]),
            flags: data[7],
        })
    }
}

/// A push to add to the receiving player's motion, in blocks a tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PushMessage(pub [f32; 3]);

impl PushMessage {
    pub const LEN: usize = 12;

    pub fn encode(self) -> [u8; Self::LEN] {
        let mut b = [0u8; Self::LEN];
        for (i, v) in self.0.iter().enumerate() {
            b[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        b
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() != Self::LEN {
            return None;
        }
        let mut v = [0f32; 3];
        for (i, out) in v.iter_mut().enumerate() {
            let x = f32::from_le_bytes([data[i * 4], data[i * 4 + 1], data[i * 4 + 2], data[i * 4 + 3]]);
            // A client trusts its server, but not a NaN.
            *out = if x.is_finite() { x.clamp(-4.0, 4.0) } else { 0.0 };
        }
        Some(Self(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anim_round_trips() {
        let m = AnimMessage { entity: 0xDEAD_BEEF, anim: 2, elapsed: 17, flags: THROW_HIT };
        assert_eq!(AnimMessage::decode(&m.encode()), Some(m));
        assert_eq!(AnimMessage::decode(&[0; 3]), None);
    }

    #[test]
    fn push_round_trips_and_clamps() {
        let m = PushMessage([0.8, 1.6, -0.8]);
        assert_eq!(PushMessage::decode(&m.encode()), Some(m));
        let wild = PushMessage([f32::NAN, 10.0, -10.0]).encode();
        assert_eq!(PushMessage::decode(&wild), Some(PushMessage([0.0, 4.0, -4.0])));
    }
}
