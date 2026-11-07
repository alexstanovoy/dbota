//! Live spectator and replay POV masks, using the full stream of a real match as the replay.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{App as Core, Intent, Net, Source, Transport};
use bota_proto::{
    ClientMsg, HeroId, ReplayHeader, ReplayRecord, Role, ServerMsg, SlotId, Team, UnitKind,
};
use clap::Parser;

use super::fog::veil_alpha;
use super::harness::{Client, Server, quiet_seat};
use crate::fog::FogGrid;

const SOON: Duration = Duration::from_secs(30);
const MAX_RECORD_BYTES: usize = 32 * 1024 * 1024;
/// Ticks a replay made by [`Spectated::save_after_falls`] shows the buildings standing, before
/// they fall.
const STANDING_TICKS: u32 = 2;
static NEXT_REPLAY: AtomicU32 = AtomicU32::new(0);

struct Recording {
    net: Net,
    bytes: Vec<u8>,
    info: Option<bota_proto::MatchInfo>,
    started: bool,
    whole: usize,
    tick: u32,
    deaths: Vec<(bota_proto::EntityId, usize, u32)>,
    /// Whether the bytes are not kept: a test that makes its own records needs the match's
    /// description and no more.
    discard: bool,
    /// Recorded no further: what comes is read and let go.
    closed: bool,
}

impl Recording {
    fn open(server: &Server, discard: bool) -> Self {
        let mut net = Net::connect(&server.address).expect("the recording spectator connects");
        net.send(&ClientMsg::hello(
            Role::Spectator,
            "recorder",
            None,
            bota_core::WIRE_FINGERPRINT,
        ))
        .expect("the spectator hello");
        net.send(&bota_client_core::catalogue())
            .expect("the catalogue");
        Self {
            net,
            bytes: bota_proto::encode_frame_to_vec(&ReplayRecord::Header(ReplayHeader::new(
                bota_core::WIRE_FINGERPRINT,
            )))
            .expect("the replay header"),
            info: None,
            started: false,
            whole: 0,
            tick: 0,
            deaths: Vec::new(),
            discard,
            closed: false,
        }
    }

    fn append(&mut self, message: ServerMsg) {
        if self.discard {
            return;
        }
        let bytes = bota_proto::encode_frame_to_vec(&ReplayRecord::Msg(message))
            .expect("a real server message encodes");
        assert!(
            self.bytes.len() + bytes.len() <= MAX_RECORD_BYTES,
            "the bounded replay fixture is full"
        );
        self.bytes.extend(bytes);
    }

    fn poll(&mut self) {
        let messages = self.net.poll();
        if self.closed {
            return;
        }
        for message in messages {
            match message {
                ServerMsg::MatchStart { info } => self.info = Some(info),
                ServerMsg::Snapshot { view } => {
                    assert!(
                        view.viewer.is_none(),
                        "the recorder receives the full stream"
                    );
                    if !self.started {
                        self.append(ServerMsg::MatchStart {
                            info: self.info.clone().expect("the match description"),
                        });
                        self.started = true;
                    }
                    self.append(ServerMsg::Snapshot { view });
                }
                ServerMsg::Events { tick, events } if self.started => {
                    let deaths: Vec<_> = events
                        .iter()
                        .filter_map(|event| match event {
                            bota_proto::EventKind::Died { unit, .. } => Some(*unit),
                            _ => None,
                        })
                        .collect();
                    self.append(ServerMsg::Events { tick, events });
                    self.whole = self.bytes.len();
                    self.tick = tick;
                    for unit in deaths {
                        assert!(self.deaths.len() < 4096, "bounded death markers");
                        self.deaths.push((unit, self.whole, tick));
                    }
                }
                _ => {}
            }
        }
    }

    fn save(&self, server: Server) -> ReplayFile {
        self.save_until(server, self.whole, self.tick)
    }

    fn save_until(&self, server: Server, end: usize, tick: u32) -> ReplayFile {
        assert!(end > 0, "a whole tick recorded");
        let path = crate::test_support::scratch_path(&format!(
            "fog_replay_{}_{}.brp",
            std::process::id(),
            NEXT_REPLAY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, &self.bytes[..end]).expect("the replay fixture written once");
        ReplayFile {
            path,
            tick,
            _server: server,
        }
    }
}

pub(super) struct ReplayFile {
    path: PathBuf,
    tick: u32,
    // Keep the harness's client turn through playback, preventing parallel replay apps.
    _server: Server,
}

impl Drop for ReplayFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(super) struct Spectated {
    _server: Server,
    pub(super) driver: Core,
    pub(super) client: Client,
    recording: Recording,
    pub(super) other: Option<Core>,
    hero: HeroId,
}

impl Spectated {
    pub(super) fn start(map: &str) -> Self {
        Self::start_with(map, HeroId::SHADOW_FIEND, false, false, &[])
    }

    /// [`Spectated::start`] for a test that makes its own records ([`Spectated::save_after_falls`]):
    /// the match is not recorded.
    pub(super) fn start_unrecorded(map: &str) -> Self {
        Self::start_with(map, HeroId::SHADOW_FIEND, false, true, &[])
    }

    pub(super) fn pair(map: &str) -> Self {
        Self::start_with(map, HeroId::SHADOW_FIEND, true, false, &[])
    }

    pub(super) fn hero(map: &str, hero: HeroId) -> Self {
        Self::start_with(map, hero, false, false, &[])
    }

    /// The match on `map` with the server's `extra` options besides these.
    pub(super) fn with_options(map: &str, extra: &[&str]) -> Self {
        Self::start_with(map, HeroId::SHADOW_FIEND, false, false, extra)
    }

    fn start_with(map: &str, hero: HeroId, pair: bool, discard: bool, extra: &[&str]) -> Self {
        let mut options = vec![
            "--map",
            map,
            "--spectators",
            "open",
            "--runes",
            "all",
            "--seed",
            "1",
        ];
        options.extend_from_slice(extra);
        let server = Server::start(2, &options);
        let args = crate::Args::parse_from(["dbota", "--addr", &server.address]);
        let driver = crate::session::open(&args)
            .expect("the driving player connects")
            .0;
        let recording = Recording::open(&server, discard);
        let client = Client::run(&["--addr", &server.address, "--spectate"]);
        let mut game = Self {
            _server: server,
            driver,
            client,
            recording,
            other: None,
            hero,
        };
        game.wait("the driving seat assigned", |driver, _| {
            driver.my_slot.is_some()
        });
        if pair {
            let mut net = Net::connect(&game._server.address).expect("the second player connects");
            net.send(&ClientMsg::hello(
                Role::Player,
                "marker",
                None,
                bota_core::WIRE_FINGERPRINT,
            ))
            .unwrap();
            net.send(&bota_client_core::catalogue()).unwrap();
            game.other = Some(Core::new(Source::Live(Box::new(net))));
        } else {
            quiet_seat(&game._server, HeroId::SHADOW_FIEND);
        }
        game.wait("the real match begins", |driver, client| {
            driver.view().is_some() && client.core().view().is_some()
        });
        game.cheat(bota_proto::Cheat::StartGame);
        game.client
            .world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_millis(100),
            ));
        game.wait("the spectator minimap ready", |_, client| {
            client.world().resource::<crate::minimap::PhotoTaken>().0
        });
        game
    }

    pub(super) fn wait(&mut self, what: &str, mut done: impl FnMut(&Core, &mut Client) -> bool) {
        let (driver, recording, other, hero) = (
            &mut self.driver,
            &mut self.recording,
            &mut self.other,
            self.hero,
        );
        self.client.within(SOON, what, |client| {
            pick_and_pump(driver, hero);
            if let Some(other) = other {
                pick_and_pump(other, HeroId::SHADOW_FIEND);
            }
            assert!(
                !driver.lost,
                "the driver lost its real server link: {:?}",
                driver.feed.last().map(|line| &line.kind)
            );
            recording.poll();
            done(driver, client)
        });
    }

    pub(super) fn next_side(&mut self, team: Team) {
        let view = self.client.core().view().expect("the match view");
        let target = view
            .players
            .iter()
            .find(|player| player.team == team)
            .expect("the watched side")
            .slot;
        let seats = view.players.len();
        for _ in 0..=seats {
            self.client.core_mut().act(Intent::NextEyes);
            if self.client.core().eyes == Some(target) {
                break;
            }
        }
        assert_eq!(self.client.core().eyes, Some(target));
        self.wait("the spectator stream switches side", |_, client| {
            client
                .core()
                .view()
                .is_some_and(|view| view.viewer == Some(team))
        });
    }

    pub(super) fn cheat(&mut self, cheat: bota_proto::Cheat) {
        let before = self
            .driver
            .view()
            .expect("the match")
            .tick
            .max(self.recording.tick)
            .max(self.client.tick());
        self.driver.act(Intent::Cheat(cheat));
        let (driver, recording, other) = (&mut self.driver, &mut self.recording, &mut self.other);
        self.client
            .within(SOON, "the tick after the cheat recorded", |_| {
                driver.pump(0.0);
                if let Some(other) = other {
                    other.pump(0.0);
                }
                recording.poll();
                recording.tick > before + 1
            });
    }

    /// Team lines are private on the live spectator socket. Preserve the real messages
    /// received by the two players, as a full server replay's Heard records do.
    pub(super) fn record_team_lines(&mut self, count: usize) {
        let (driver, other, recording) = (&mut self.driver, &mut self.other, &mut self.recording);
        let mut lines = Vec::new();
        self.client.within(
            SOON,
            "both players received their real team messages",
            |_| {
                driver.pump(0.0);
                other.as_mut().expect("the other player").pump(0.0);
                recording.poll();
                lines = driver
                    .chat_lines()
                    .chain(other.as_ref().unwrap().chat_lines())
                    .map(|line| line.heard.clone())
                    .collect();
                lines.len() == count
            },
        );
        for line in lines {
            let bytes = bota_proto::encode_frame_to_vec(&ReplayRecord::Heard(line))
                .expect("a real team message");
            assert!(recording.bytes.len() + bytes.len() <= MAX_RECORD_BYTES);
            recording.bytes.extend(bytes);
        }
        recording.whole = recording.bytes.len();
    }

    pub(super) fn save(self) -> ReplayFile {
        self.recording.save(self._server)
    }

    pub(super) fn until_death(&mut self, unit: bota_proto::EntityId) {
        let (driver, recording, other) = (&mut self.driver, &mut self.recording, &mut self.other);
        self.client
            .within(SOON, "the victim's death recorded", |_| {
                driver.pump(0.0);
                if let Some(other) = other {
                    other.pump(0.0);
                }
                recording.poll();
                recording.deaths.iter().any(|(id, _, _)| *id == unit)
            });
    }

    pub(super) fn save_through_death(self, unit: bota_proto::EntityId) -> ReplayFile {
        let (_, end, tick) = self
            .recording
            .deaths
            .iter()
            .find(|(id, _, _)| *id == unit)
            .copied()
            .expect("the death was recorded");
        self.recording.save_until(self._server, end, tick)
    }

    /// A replay of the match as the watching spectator sees it now, with `fallen` standing for
    /// a moment and then brought down, each told by its death and its fall, as the server tells
    /// them; the replay ends `after` ticks after, the fall told `late` ticks late. The tick the first of them is gone on comes
    /// with it.
    pub(super) fn save_after_falls(
        self,
        fallen: &[bota_proto::EntityId],
        after: u32,
        late: u32,
    ) -> (ReplayFile, u32) {
        let alive = self
            .client
            .core()
            .view()
            .expect("a view of the match")
            .clone();
        let info = self.recording.info.clone().expect("the match description");
        let mut gone = alive.clone();
        let falls: Vec<bota_proto::EventKind> = fallen
            .iter()
            .flat_map(|id| {
                let unit = alive.unit(*id).expect("a standing building");
                [
                    bota_proto::EventKind::Died {
                        unit: *id,
                        killer: None,
                        denied: false,
                        gold: 0,
                        bounty: None,
                        assists: 0,
                        awards: None,
                        credited_hero: None,
                    },
                    bota_proto::EventKind::StructureDestroyed {
                        unit: *id,
                        team: unit.team,
                        kind: unit.kind,
                        lane: Some(0),
                        tier: Some(1),
                        ranged: Some(false),
                    },
                ]
            })
            .collect();
        gone.units.retain(|unit| !fallen.contains(&unit.id));
        let mut bytes = Vec::new();
        bota_proto::encode_frame(
            &ReplayRecord::Header(ReplayHeader::new(bota_core::WIRE_FINGERPRINT)),
            &mut bytes,
        )
        .expect("the header encodes");
        let mut put = |message: ServerMsg| {
            bota_proto::encode_frame(&ReplayRecord::Msg(message), &mut bytes)
                .expect("a message encodes");
        };
        put(ServerMsg::MatchStart { info });
        let first = alive.tick;
        let mut last = first;
        // `late` ticks pass between the last view with them standing and the first without, as when
        // the server coalesces ticks; their fall is told with the later view, but as of the tick
        // they fell on.
        let fell_on = first + STANDING_TICKS;
        for ahead in (0..STANDING_TICKS).chain(STANDING_TICKS + late..STANDING_TICKS + late + after)
        {
            let tick = first + ahead;
            let mut view = if ahead < STANDING_TICKS {
                alive.clone()
            } else {
                gone.clone()
            };
            view.tick = tick;
            put(ServerMsg::Snapshot { view });
            // The server tells each tick's events once, and the fall's with the first view that
            // lacks the buildings.
            let (told_for, events) = if ahead == STANDING_TICKS + late {
                (fell_on, falls.clone())
            } else {
                (tick, Vec::new())
            };
            put(ServerMsg::Events {
                tick: told_for,
                events,
            });
            last = tick;
        }
        let path = crate::test_support::scratch_path(&format!(
            "fall_replay_{}_{}.brp",
            std::process::id(),
            NEXT_REPLAY.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, &bytes).expect("the replay fixture written once");
        (
            ReplayFile {
                path,
                tick: last,
                _server: self._server,
            },
            first + STANDING_TICKS,
        )
    }

    /// Ends the replay at the last whole tick recorded; the match goes on unrecorded, the
    /// replay saved by [`Self::save`].
    pub(super) fn stop_recording(&mut self) {
        self.recording.closed = true;
    }

    pub(super) fn order(&mut self, order: bota_proto::Order) {
        let before = self
            .recording
            .tick
            .max(self.driver.view().expect("the match").tick);
        self.driver.send_order(order);
        let (driver, recording, other) = (&mut self.driver, &mut self.recording, &mut self.other);
        self.client
            .within(SOON, "the order's next tick recorded", |_| {
                driver.pump(0.0);
                if let Some(other) = other {
                    other.pump(0.0);
                }
                recording.poll();
                recording.tick > before + 1
            });
    }
}

fn pick_and_pump(core: &mut Core, hero: HeroId) {
    core.pump(0.0);
    if core.view().is_none() && core.my_slot.is_some() && !core.ready {
        core.act(Intent::PickHero(hero.0));
        core.act(Intent::ToggleReady);
    }
}

fn side_points(client: &Client, team: Team) -> Option<(Vec2, Vec2)> {
    let view = client.core().view()?;
    let own = view
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.team == team)?
        .pos;
    let enemy = view
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Tower && u.team != team)
        .max_by_key(|u| u.pos.distance_squared(own))?
        .pos;
    Some((
        Vec2::new(own.x.to_f32(), own.y.to_f32()),
        Vec2::new(enemy.x.to_f32(), enemy.y.to_f32()),
    ))
}

fn mask_ready(client: &Client, team: Team) -> bool {
    let grid = client.world().resource::<FogGrid>();
    let Some((own, enemy)) = side_points(client, team) else {
        return false;
    };
    grid.team == Some(team)
        && grid.worked_out_at().is_some()
        && client.world().resource::<crate::minimap::PhotoTaken>().0
        && grid.lit(own.x, own.y)
        && !grid.lit(enemy.x, enemy.y)
        && veil_alpha(client, own) < 8
        && veil_alpha(client, enemy) > 170
}

/// Check the very first frame of the new POV, before a later sight task can hide the bug.
fn assert_previous_side_hidden(client: &Client, team: Team) {
    let (_, enemy) = side_points(client, team).expect("both sides' buildings known");
    let grid = client.world().resource::<FogGrid>();
    assert_eq!(grid.team, Some(team));
    assert!(
        !grid.lit(enemy.x, enemy.y),
        "the new {team:?} POV still exposes the previous side's tower vision"
    );
    assert!(
        veil_alpha(client, enemy) > 170,
        "the minimap still exposes the previous side's tower vision"
    );
}

#[test]
fn spectator_pov_switch_never_exposes_the_previous_sides_vision() {
    for map in ["0", "2"] {
        let mut game = Spectated::start(map);
        assert!(game.client.core().my_slot.is_none());
        game.next_side(Team::Radiant);
        game.wait("Radiant sight settled", |_, client| {
            mask_ready(client, Team::Radiant)
        });
        for team in [Team::Dire, Team::Radiant] {
            game.next_side(team);
            assert_previous_side_hidden(&game.client, team);
            game.wait("the new side's own sight revealed", |_, client| {
                mask_ready(client, team)
            });
        }
    }
}

fn recorded_match(map: &str) -> ReplayFile {
    let mut game = Spectated::start(map);
    let (driver, recording) = (&mut game.driver, &mut game.recording);
    game.client
        .within(SOON, "a whole spectator tick recorded", |_| {
            driver.pump(0.0);
            recording.poll();
            recording.tick > 0
        });
    game.save()
}

#[test]
fn replay_keyboard_pov_switch_masks_old_vision_in_the_same_update() {
    let file = recorded_match("2");
    let mut client = replay_from_side(&file, 0);
    client.within(SOON, "Radiant's replay sight ready", |c| {
        mask_ready(c, Team::Radiant)
    });
    client.key(KeyCode::Tab, bevy::input::ButtonState::Pressed);
    assert_eq!(
        client.core().my_slot,
        Some(SlotId(1)),
        "the actual Tab binding switched eyes"
    );
    assert_previous_side_hidden(&client, Team::Dire);
    client.frames(1);
    assert_eq!(
        client.core().my_slot,
        Some(SlotId(1)),
        "a held Tab must not repeat last frame's intent"
    );
    client.key(KeyCode::Tab, bevy::input::ButtonState::Released);
    assert_eq!(client.core().my_slot, Some(SlotId(1)));
}

pub(super) fn replay_from_side(file: &ReplayFile, slot: u8) -> Client {
    replay_from_side_with(file, slot, &[])
}

pub(super) fn replay_from_side_with(file: &ReplayFile, slot: u8, extra: &[&str]) -> Client {
    replay_observed(file, Some(slot), extra, |_| {})
}

pub(super) fn replay_observed(
    file: &ReplayFile,
    slot: Option<u8>,
    extra: &[&str],
    setup: impl FnOnce(&mut Client),
) -> Client {
    let slot = slot.map(|slot| slot.to_string());
    let mut args = vec!["--replay", file.path.to_str().expect("a UTF-8 path")];
    if let Some(slot) = &slot {
        args.extend(["--pov", slot.as_str()]);
    }
    args.extend_from_slice(extra);
    let mut client = Client::run(&args);
    setup(&mut client);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(100),
        ));
    client.within(SOON, "the replay particle library ready", |c| {
        c.world()
            .get_resource::<crate::particles::Library>()
            .is_some()
    });
    client.within(SOON, "the recorded match played", |c| {
        c.core().view().is_some_and(|view| view.tick >= file.tick)
    });
    if let Source::Replay(player) = &mut client.core_mut().source {
        player.paused = true;
    }
    client
}

#[test]
fn replay_each_side_and_mid_replay_pov_switch_keep_vision_private() {
    for map in ["0", "2"] {
        let file = recorded_match(map);
        for (slot, team, other) in [
            (0, Team::Radiant, Team::Dire),
            (1, Team::Dire, Team::Radiant),
        ] {
            let mut client = replay_from_side(&file, slot);
            client.within(SOON, "the replay side's sight revealed", |c| {
                mask_ready(c, team)
            });
            if slot == 0 {
                client.core_mut().act(Intent::NextEyes);
            } else {
                client.core_mut().act(Intent::NextEyes);
                client.core_mut().act(Intent::NextEyes);
            }
            client.frames(1);
            assert_eq!(
                client.core().my_slot,
                Some(SlotId(if slot == 0 { 1 } else { 0 }))
            );
            assert_previous_side_hidden(&client, other);
            client.within(SOON, "the switched replay side's sight revealed", |c| {
                mask_ready(c, other)
            });
        }
    }
}
