//! Mutant Monsters for Horizon: the native (trait mod) half.
//!
//! A port of Mutant Monsters by shcott21, Chumbanotz, Fuzs and tdstress
//! (github.com/Fuzss/mutantmonsters, AGPL-3.0; see LICENSE and NOTICE.md in
//! the mod folder). The Mutant Zombie's model, poses, goals, attacks, lives,
//! the Hulk Hammer and Chemical X are translated from that Java code; the
//! original Java this was translated from is kept in `upstream/`.
//!
//! The launcher links this crate into the game and calls `hooks()`
//! (docs/native-mods.md, "Trait mods"). The same hooks serve a dedicated
//! server (the server hooks fire) and a client (the drawing hooks fire).
pub mod logic;
pub mod net;
pub mod pose;

mod brain;
mod client;
pub mod creeper;
mod goals;
mod model;
mod server;
pub mod skeleton;

use horizon_client::mod_api::{
    ClientTickPostEvent, EntityTickListenedEvent, EntityTickPostEvent, Flow, Hooks, Host,
    HearsProjectileImpactsEvent, LayerDefinitionsEvent, LevelTickPostEvent, LivingConversionPreEvent,
    LivingDeathEvent, LivingIncomingDamageEvent, MobSubmitEvent, NetFromServerEvent,
    ProjectileImpactEvent, RegisterGoalsEvent, ServerTickEvent, UseItemOnEvent,
};

pub fn hooks() -> Box<dyn Hooks> {
    Box::new(Mod)
}

struct Mod;

impl Hooks for Mod {
    // Both sides: a client builds its copies of mobs with the same goals.
    fn register_goals(&self, event: &mut RegisterGoalsEvent<'_>) {
        goals::register(event.mob);
    }

    // Server side.
    fn entity_tick_listened(&self, host: &mut dyn Host, event: &mut EntityTickListenedEvent) {
        server::entity_tick_listened(host, event);
    }

    fn entity_tick_post(&self, host: &mut dyn Host, event: &mut EntityTickPostEvent<'_>) {
        server::entity_tick_post(host, event);
    }

    fn server_tick(&self, host: &mut dyn Host, event: &mut ServerTickEvent<'_>) {
        server::server_tick(host, event);
    }

    fn level_tick_post(&self, host: &mut dyn Host, event: &mut LevelTickPostEvent<'_>) {
        server::level_tick_post(host, event);
    }

    fn living_death(&self, host: &mut dyn Host, event: &mut LivingDeathEvent<'_>) -> Flow {
        server::living_death(host, event)
    }

    fn living_incoming_damage(&self, event: &mut LivingIncomingDamageEvent<'_>) -> Flow {
        server::living_incoming_damage(event)
    }

    fn living_conversion_pre(&self, _host: &mut dyn Host, event: &mut LivingConversionPreEvent<'_>) -> Flow {
        server::living_conversion_pre(event)
    }

    fn use_item_on(&self, host: &mut dyn Host, event: &mut UseItemOnEvent<'_>) -> Flow {
        server::use_item_on(host, event)
    }

    fn hears_projectile_impacts(&self, _host: &dyn Host, event: &mut HearsProjectileImpactsEvent) {
        server::hears_projectile_impacts(event);
    }

    fn projectile_impact(&self, host: &mut dyn Host, event: &mut ProjectileImpactEvent<'_>) -> Flow {
        server::projectile_impact(host, event)
    }

    // Client side.
    fn layer_definitions(&self, event: &mut LayerDefinitionsEvent<'_>) {
        client::layer_definitions(event);
    }

    fn mob_submit(&self, host: &dyn Host, event: &mut MobSubmitEvent<'_>) -> Flow {
        client::mob_submit(host, event)
    }

    fn net_from_server(&self, host: &mut dyn Host, event: &mut NetFromServerEvent<'_>) {
        client::net_from_server(host, event);
    }

    fn client_tick_post(&self, _host: &mut dyn Host, _event: &mut ClientTickPostEvent) {
        client::tick();
    }
}
