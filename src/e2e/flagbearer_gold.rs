//! Both allied players receive the independently rolled flagbearer reward in a real match.

use std::time::{Duration, Instant};

use bota_client_core::{App as Core, Intent, Net, Source, Transport};
use bota_proto::{
    Cheat, HeroId, ItemId, ModifierSpec, Order, SlotId, Target, Team, UnitKind, Vec2,
};

use super::harness::{Client, Server};

struct Allies {
    first: Client,
    second: Client,
    opponent: Core,
    // Last: retain the harness's turn until both large client apps have been dropped.
    server: Server,
}

impl Allies {
    fn start() -> Self {
        let server = Server::start(
            3,
            &[
                "--map",
                "0",
                "--seed",
                "5",
                "--runes",
                "none",
                "--objectives",
                "none",
                "--pick-secs",
                "1000",
                "--ack-timeout-ticks",
                "100000",
            ],
        );
        let mut first = Client::run_with_core(
            &["--addr", &server.address, "--name", "earner"],
            player_core(&server, "earner"),
        );
        first.until("the first seat assigned", |client| {
            client.core().my_slot == Some(SlotId(0))
        });
        ready(first.core_mut());
        let mut opponent = player_core(&server, "opponent");
        first.until("the opposing seat assigned", |_| {
            ready(&mut opponent);
            opponent.my_slot == Some(SlotId(1))
        });
        let second = Client::run_with_core(
            &["--addr", &server.address, "--name", "ally"],
            player_core(&server, "ally"),
        );
        let mut game = Self {
            first,
            second,
            opponent,
            server,
        };
        game.wait("both allied heroes in the match", |game| {
            game.first.core().view().is_some() && game.second.core().view().is_some()
        });
        assert_eq!(game.second.core().my_slot, Some(SlotId(2)));
        assert_eq!(game.first.me().team, Team::Radiant);
        assert_eq!(game.first.me().team, game.second.me().team);
        game
    }

    fn wait(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            ready(self.first.core_mut());
            ready(self.second.core_mut());
            ready(&mut self.opponent);
            if done(self) {
                return;
            }
            assert!(Instant::now() < deadline, "{what}");
            std::thread::yield_now();
        }
    }

    fn cheat(&mut self, who: usize, cheat: Cheat) {
        let before = self.first.tick().max(self.second.tick());
        let client = if who == 0 {
            &mut self.first
        } else {
            &mut self.second
        };
        client.core_mut().reject = None;
        client.core_mut().act(Intent::Cheat(cheat));
        self.wait("both clients receive the cheat's next tick", |game| {
            game.first.tick() > before + 1 && game.second.tick() > before + 1
        });
        let client = if who == 0 { &self.first } else { &self.second };
        assert!(
            client.core().reject.is_none(),
            "cheat refused: {:?}",
            client.core().reject
        );
    }
}

fn player_core(server: &Server, name: &str) -> Core {
    let mut net = Net::connect(&server.address).expect("an independent player connects");
    net.send(&bota_proto::ClientMsg::hello(
        bota_proto::Role::Player,
        name,
        None,
        bota_core::WIRE_FINGERPRINT,
    ))
    .expect("player hello");
    net.send(&bota_client_core::catalogue()).expect("catalogue");
    net.send(&bota_proto::ClientMsg::Compress(bota_proto::Codec::Deflate))
        .expect("compression");
    Core::new(Source::Live(Box::new(net)))
}

fn ready(core: &mut Core) {
    core.pump(0.0);
    if core.view().is_none() && core.my_slot.is_some() && !core.ready {
        core.act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
        core.act(Intent::ToggleReady);
    }
}

#[derive(Clone, Copy)]
struct Purse {
    gold: i32,
    passive: i32,
    last_hits: u16,
}

fn purse(client: &Client) -> Purse {
    let slot = client.core().my_slot.expect("seated");
    let player = client.view().player(slot).expect("own row");
    let horn = client
        .core()
        .statics()
        .expect("match")
        .metadata()
        .pregame_ticks;
    Purse {
        gold: player.gold.expect("own wallet"),
        passive: bota_core::passive_gold_by(client.view().game_tick().saturating_sub(horn)),
        last_hits: player.creep_score.expect("own creep score").last_hits,
    }
}

fn received(before: Purse, client: &Client) -> i32 {
    let after = purse(client);
    after.gold - before.gold - (after.passive - before.passive)
}

fn check_wallet_hud(client: &mut Client) {
    client.until(
        "the actual shop-corner gold label agrees with its player's balance",
        |client| {
            crate::shop::drawn_gold(client.world_mut()) == Some(purse(client).gold.to_string())
        },
    );
}

#[test]
fn both_allied_heroes_receive_the_flagbearer_bonus_and_only_the_last_hitter_gets_the_kill_bounty() {
    let mut game = Allies::start();
    let flag = enemy_flagbearer(&mut game);
    approach_and_prepare(&mut game, &flag);
    game.server.frozen(|| {
        check_wallet_hud(&mut game.first);
        check_wallet_hud(&mut game.second);
    });
    let before = [purse(&game.first), purse(&game.second)];
    game.first.core_mut().send_order_to(
        None,
        Order::Attack {
            target: Target::Unit(flag.id),
        },
    );
    game.wait("both clients see the ordered flagbearer kill", |game| {
        game.first.view().unit(flag.id).is_none()
            && game.second.view().unit(flag.id).is_none()
            && purse(&game.first).last_hits > before[0].last_hits
    });
    game.server.frozen(|| {
        check_wallet_hud(&mut game.first);
        check_wallet_hud(&mut game.second);
        let bonus = received(before[1], &game.second);
        let total = received(before[0], &game.first);
        assert!((44..=49).contains(&bonus), "7.41f bonus: {bonus}");
        assert!(
            (34..=39).contains(&(total - bonus)),
            "independent kill bounty: {}",
            total - bonus
        );
        assert_eq!(purse(&game.first).last_hits, before[0].last_hits + 1);
        assert_eq!(purse(&game.second).last_hits, before[1].last_hits);
        eprintln!(
            "flagbearer wallets: killer={total}, ally={bonus}, kill component={}",
            total - bonus
        );
    });
}

fn enemy_flagbearer(game: &mut Allies) -> bota_proto::UnitView {
    game.cheat(0, Cheat::Invulnerable { on: true });
    game.cheat(1, Cheat::Invulnerable { on: true });
    game.cheat(0, Cheat::AllVision { on: true });
    game.cheat(0, Cheat::StartGame);
    game.wait("the first enemy flagbearer wave", |game| {
        game.first
            .view()
            .units
            .iter()
            .any(|unit| unit.kind == UnitKind::CreepFlagbearer && unit.team == Team::Dire)
    });
    game.cheat(0, Cheat::CreepSpawning { on: false });
    game.first
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::CreepFlagbearer && unit.team == Team::Dire)
        .expect("enemy flag")
        .clone()
}

fn approach_and_prepare(game: &mut Allies, flag: &bota_proto::UnitView) {
    game.cheat(
        0,
        Cheat::Teleport {
            at: flag.pos + Vec2::from_ints(-700, 0),
        },
    );
    game.cheat(
        1,
        Cheat::Teleport {
            at: flag.pos + Vec2::from_ints(-900, 0),
        },
    );
    game.cheat(
        0,
        Cheat::Item {
            item: ItemId::DEMON_EDGE,
        },
    );
    game.cheat(
        0,
        Cheat::ApplyModifier {
            target: Target::None,
            spec: ModifierSpec {
                physical_damage: 40000,
                ..ModifierSpec::NOMINAL
            },
            ticks: 3000,
        },
    );
    game.cheat(
        0,
        Cheat::ApplyModifier {
            target: Target::Unit(flag.id),
            spec: ModifierSpec {
                max_hp: 2500,
                move_speed: 2500,
                ..ModifierSpec::NOMINAL
            },
            ticks: 3000,
        },
    );
}
