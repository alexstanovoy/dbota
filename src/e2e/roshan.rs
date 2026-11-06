//! Roshan in a real server's match, as the client draws him.
use super::harness::{Client, Server};
use crate::units::{ModelBank, Unit};
use bota_proto::{Cheat, HeroId, UnitKind};

#[test]
fn roshan_stands_in_the_pit_as_his_imported_model_with_a_health_bar() {
    let server = Server::start(1, &["--objectives", "roshan", "--immortal-heroes"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 100, pit.y.to_int()));
    client.until("Roshan drawn in his pit with a health bar", |client| {
        let Some(roshan) = client
            .view()
            .units
            .iter()
            .find(|unit| unit.kind == UnitKind::Roshan)
        else {
            return false;
        };
        assert!(roshan.pos.within(pit, bota_core::rules::units(300)));
        let Some(body) = client.body(roshan.id) else {
            return false;
        };
        let world = client.world();
        let Some(model) = world.get::<Unit>(body).and_then(|unit| unit.model) else {
            return false;
        };
        world.resource::<ModelBank>().0[model].model.logical == "neutral.roshan.model"
            && world.resource::<crate::bars::Bars>().has(body)
    });
}
