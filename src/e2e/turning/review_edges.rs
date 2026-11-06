//! Presentation failures at the gate's boundaries, against a real match.

use super::*;
use crate::units::{Attachments, Unit, UnitIndex};
use std::collections::{HashSet, VecDeque};

fn attack(client: &mut Client, enemy: &UnitView) {
    client.core_mut().act(Intent::AttackAt {
        at: (enemy.pos.x.to_f32(), enemy.pos.y.to_f32()),
        unit: Some(enemy.id),
    });
}

fn swing(client: &Client) -> Option<bota_proto::SwingView> {
    client
        .view()
        .aggro
        .iter()
        .find(|a| a.unit == client.me().id)
        .and_then(|a| a.swing)
}

fn clip(client: &Client) -> &'static str {
    client
        .world()
        .get::<crate::anim::Pose>(client.body(client.me().id).expect("hero body"))
        .expect("hero pose")
        .clip()
}

#[derive(Resource)]
struct MissingTarget {
    id: EntityId,
    saved: Option<Entity>,
    hidden_frames: u32,
}

fn hide_target(mut missing: ResMut<MissingTarget>, mut index: ResMut<UnitIndex>) {
    missing.saved = index.0.remove(&missing.id);
    missing.hidden_frames += u32::from(missing.saved.is_some());
}

fn restore_target(mut missing: ResMut<MissingTarget>, mut index: ResMut<UnitIndex>) {
    if let Some(body) = missing.saved.take() {
        index.0.insert(missing.id, body);
    }
}

#[test]
fn c_turn_missing_target_body_uses_its_view_position_to_finish_the_turn() {
    let (_server, mut client, enemy) = attack_scene(HeroId::SHADOW_FIEND, Spot::from_ints(-120, 0));
    client.world_mut().insert_resource(MissingTarget {
        id: enemy.id,
        saved: None,
        hidden_frames: 0,
    });
    client
        .app
        .add_systems(PostUpdate, hide_target.before(crate::anim::NoteChannels));
    client.app.add_systems(Last, restore_target);
    let mut samples = VecDeque::new();
    samples.extend(std::iter::repeat_n(0.0, 4));
    let id = client.me().id;
    client.world_mut().insert_resource(FacingLag {
        id,
        samples,
        frames: 4,
    });
    client
        .app
        .add_systems(Update, lag_facing.after(crate::units::Placed));
    attack(&mut client, &enemy);
    let (mut waited, mut due) = (false, None);
    for _ in 0..40 {
        client.frames(1);
        if hidden_target_frame(&mut client, &enemy, &mut waited, &mut due) {
            return;
        }
    }
    panic!("Shadow Fiend's attack with an unindexed target never launched");
}

fn hidden_target_frame(
    client: &mut Client,
    enemy: &UnitView,
    waited: &mut bool,
    due: &mut Option<u32>,
) -> bool {
    assert!(
        client.view().unit(enemy.id).is_some(),
        "the target stays in the view"
    );
    if let Some(swing) = swing(client) {
        due.get_or_insert(client.tick() + swing.strike_in);
        let wanted = bota_core::facing_towards(client.me().pos, enemy.pos);
        let gap = delta(drawn_facing(client), radians(i32::from(wanted.brads))).abs();
        let remaining = (swing.strike_in as f32 - client.core().smooth.alpha()) / 30.0;
        if gap > radians(i32::from(bota_core::rules::ATTACK_ANGLE_BRADS)) && remaining > 1.0 / 30.0
        {
            *waited = true;
            assert!(
                drawn_attack(client).is_none(),
                "a known target position must gate the still-turning model"
            );
        }
    }
    if !client
        .view()
        .projectiles
        .iter()
        .any(|p| p.source == Some(client.me().id))
    {
        return false;
    }
    assert!(
        *waited,
        "the fixture must exercise a delayed turn with no indexed target body"
    );
    assert!(client.world().resource::<MissingTarget>().hidden_frames > 0);
    assert_eq!(
        Some(client.tick()),
        *due,
        "the fallback preserves the strike tick"
    );
    let (time, point) = drawn_attack(client).expect("the release is drawn");
    assert!(
        time + 0.001 >= point,
        "the visible attack reaches its point"
    );
    true
}

#[derive(Resource)]
struct FacingLag {
    id: EntityId,
    samples: VecDeque<f32>,
    frames: usize,
}

fn lag_facing(mut lag: ResMut<FacingLag>, index: Res<UnitIndex>, mut units: Query<&mut Unit>) {
    let Some(body) = index.0.get(&lag.id) else {
        return;
    };
    let Ok(mut unit) = units.get_mut(*body) else {
        return;
    };
    lag.samples.push_back(unit.facing);
    if lag.samples.len() > lag.frames {
        lag.samples.pop_front();
    }
    unit.facing = *lag.samples.front().expect("a current or previous facing");
}

#[derive(Resource)]
struct SnapshotElapsed(f32);

fn fixed_snapshot_interval(
    mut session: NonSendMut<crate::session::Session>,
    elapsed: Res<SnapshotElapsed>,
) {
    session.0.smooth.interval = 1.0 / 30.0;
    session.0.smooth.since = session.0.smooth.interval * elapsed.0;
}

fn max_speed(client: &mut Client) {
    client.cheat(Cheat::Levels { count: 29 });
    let item = bota_core::item_by_key("item_butterfly").expect("Butterfly");
    for _ in 0..6 {
        client.cheat(Cheat::Item { item });
    }
    assert_eq!(
        bota_core::attack_gain(client.me().attack_speed),
        7000,
        "the real inventory reaches the 700 attack-speed cap"
    );
    assert!(
        client.me().attack_point <= 72,
        "the effective windup is at most 72 ms"
    );
}

fn check_fast_windup(hero: HeroId, elapsed: f32) {
    let scenario = match hero {
        HeroId::SHADOW_FIEND => "Shadow Fiend 180-degree fast windup",
        HeroId::PUDGE => "Pudge 180-degree fast windup",
        _ => panic!("unsupported fast-windup scenario"),
    };
    let (_server, mut client, enemy) = attack_scene(hero, Spot::from_ints(-120, 0));
    max_speed(&mut client);
    // Inject bounded presentation latency, retaining real previously drawn
    // orientations while the server and its strike schedule continue normally.
    let id = client.me().id;
    let mut samples = VecDeque::new();
    samples.extend(std::iter::repeat_n(0.0, 6));
    client.world_mut().insert_resource(FacingLag {
        id,
        samples,
        frames: 6,
    });
    client.world_mut().insert_resource(SnapshotElapsed(elapsed));
    client
        .app
        .add_systems(Update, lag_facing.after(crate::units::Placed));
    client.app.add_systems(
        PreUpdate,
        fixed_snapshot_interval.after(crate::session::pump),
    );
    attack(&mut client, &enemy);
    let (mut began, mut due) = (false, None);
    for _ in 0..30 {
        client.frames(1);
        if let Some(swing) = swing(&client) {
            due.get_or_insert(client.tick() + swing.strike_in);
            let remaining = (swing.strike_in as f32 - elapsed) / 30.0;
            if remaining <= 1.0 / 30.0 {
                assert!(
                    drawn_attack(&client).is_some(),
                    "{scenario} elapsed={elapsed}: open on the first frame within one snapshot interval of the strike"
                );
            }
            if !began && let Some((time, _)) = drawn_attack(&client) {
                let wanted = bota_core::facing_towards(client.me().pos, enemy.pos);
                let gap = delta(drawn_facing(&mut client), radians(i32::from(wanted.brads))).abs();
                assert!(
                    gap > radians(i32::from(bota_core::rules::ATTACK_ANGLE_BRADS)),
                    "{scenario} display really trails the permitted angle"
                );
                assert!(
                    remaining <= 1.0 / 30.0,
                    "{scenario} only the final snapshot interval forces the gate open"
                );
                assert!(
                    time > 0.0,
                    "{scenario} a late clip begins in its later part"
                );
                began = true;
            }
        }
        let struck = client
            .view()
            .projectiles
            .iter()
            .any(|p| p.source == Some(id))
            || client
                .view()
                .unit(enemy.id)
                .is_some_and(|u| u.hp < enemy.hp);
        if struck {
            assert!(
                began,
                "{scenario} at700 IAS: a short windup must animate despite presentation lag"
            );
            assert_eq!(
                Some(client.tick()),
                due,
                "{scenario} the strike is not delayed"
            );
            let (time, point) =
                drawn_attack(&client).expect("the short clip is drawn on the strike frame");
            assert!(
                time + 0.001 >= point,
                "{scenario} the strike cannot precede the clip point"
            );
            return;
        }
    }
    panic!("{scenario} max-speed attack never struck");
}

#[test]
fn c_turn_shadow_fiend_700_attack_speed_keeps_a_short_windup_under_display_lag() {
    check_fast_windup(HeroId::SHADOW_FIEND, 0.0);
}

#[test]
fn c_turn_pudge_700_attack_speed_keeps_a_short_windup_under_display_lag() {
    check_fast_windup(HeroId::PUDGE, 0.0);
}

#[test]
fn c_turn_shadow_fiend_700_attack_speed_counts_elapsed_time_between_snapshots() {
    check_fast_windup(HeroId::SHADOW_FIEND, 1.0);
}

#[test]
fn c_turn_pudge_700_attack_speed_counts_elapsed_time_between_snapshots() {
    check_fast_windup(HeroId::PUDGE, 1.0);
}

fn hands(client: &Client) -> [Vec3; 2] {
    let body = client.body(client.me().id).expect("hero");
    let attachments = client
        .world()
        .get::<Attachments>(body)
        .expect("hero attack attachments");
    attachments.attack.map(|node| {
        client
            .world()
            .get::<GlobalTransform>(node.expect("an attack hand"))
            .expect("placed hand")
            .translation()
    })
}

fn assert_launch_hand(client: &Client, expected: Vec3, label: &str) {
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect("hero.nevermore.attack")
        .expect("attack effect");
    let playing = client.world().resource::<crate::particles::Playing>();
    let shot = playing
        .instances_of(effect)
        .min_by(|a, b| a.nodes[0].age.total_cmp(&b.nodes[0].age))
        .expect("a drawn projectile");
    let cp = shot.nodes[0].control_points[0].position;
    let expected = Vec3::new(expected.x, -expected.z, expected.y);
    assert!(
        cp.distance(expected) < 0.01,
        "{label}: actual projectile launch uses the chosen attack hand: {cp:?}/{expected:?}"
    );
}

fn check_speed_change(increase: bool) {
    let (_server, mut client, enemy) = attack_scene(HeroId::SHADOW_FIEND, Spot::from_ints(120, 0));
    let item = bota_core::item_by_key("item_butterfly").expect("Butterfly");
    if !increase {
        client.cheat(Cheat::Item { item });
    }
    attack(&mut client, &enemy);
    client.until(
        "Shadow Fiend forward-attack scenario first windup drawn",
        |c| swing(c).is_some() && clip(c) == "attack",
    );
    let initial = swing(&client).expect("windup");
    let before_due = client.tick() + initial.strike_in;
    let before_speed = client.me().attack_speed;
    if increase {
        client.core_mut().act(Intent::Cheat(Cheat::Item { item }));
    } else {
        client.core_mut().send_order(bota_proto::Order::Swap {
            from: bota_proto::ItemSlot(0),
            to: bota_proto::ItemSlot(6),
        });
    }
    let (mut changed, mut shifted, mut due) = (false, false, before_due);
    for _ in 0..30 {
        let hand = hands(&client)[0];
        client.frames(1);
        if let Some(swing) = swing(&client) {
            assert_eq!(swing.target, enemy.id);
            due = client.tick() + swing.strike_in;
            shifted |= due.abs_diff(before_due) > 1;
            changed |= client.me().attack_speed != before_speed;
            assert_eq!(
                clip(&client),
                "attack",
                "Shadow Fiend forward-attack scenario increase={increase}: speed changes do not cut or flip an ongoing windup"
            );
        }
        if client
            .view()
            .projectiles
            .iter()
            .any(|p| p.source == Some(client.me().id))
        {
            assert!(
                changed && shifted,
                "Shadow Fiend forward-attack scenario increase={increase}: the real speed and deadline both changed by more than one tick"
            );
            assert_eq!(
                client.tick(),
                due,
                "Shadow Fiend forward-attack scenario new deadline remains authoritative"
            );
            assert_eq!(
                clip(&client),
                "attack",
                "Shadow Fiend forward-attack scenario the release keeps the same clip and hand"
            );
            assert_launch_hand(
                &client,
                hand,
                "Shadow Fiend forward-attack scenario mid-windup speed change",
            );
            let (time, point) = drawn_attack(&client).expect("release clip");
            assert!(
                time + 0.001 >= point,
                "Shadow Fiend forward-attack scenario clip point precedes the launch"
            );
            return;
        }
    }
    panic!(
        "Shadow Fiend forward-attack scenario increase={increase}: the speed-changed windup never struck"
    );
}

#[test]
fn c_turn_attack_speed_increase_preserves_the_current_clip_and_launch_hand() {
    check_speed_change(true);
}

#[test]
fn c_turn_attack_speed_decrease_preserves_the_current_clip_and_launch_hand() {
    check_speed_change(false);
}

#[test]
fn c_turn_shadow_fiend_alternates_attack2_and_its_actual_launch_hand() {
    let (_server, mut client, enemy) = attack_scene(HeroId::SHADOW_FIEND, Spot::from_ints(120, 0));
    attack(&mut client, &enemy);
    let mut seen = HashSet::new();
    for _ in 0..100 {
        let hand = hands(&client);
        client.frames(1);
        let new = client
            .view()
            .projectiles
            .iter()
            .filter(|p| p.source == Some(client.me().id))
            .any(|p| seen.insert(p.id));
        if new {
            let second = seen.len() == 2;
            assert_eq!(
                clip(&client),
                if second { "attack2" } else { "attack" },
                "Shadow Fiend forward-attack scenario alternating strike clip"
            );
            assert_launch_hand(
                &client,
                hand[usize::from(second)],
                "Shadow Fiend forward-attack scenario alternating attack",
            );
            let (time, point) = drawn_attack(&client).expect("release pose");
            assert!(
                time + 0.001 >= point,
                "Shadow Fiend forward-attack scenario alternating strike follows its clip point"
            );
            if second {
                return;
            }
        }
    }
    panic!("Shadow Fiend forward-attack scenario did not show two alternating strikes");
}

#[test]
fn c_turn_a_moving_target_does_not_restart_the_current_windup() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let server = Server::start(2, &[]);
    let go = Arc::new(AtomicBool::new(false));
    let moving = go.clone();
    let ready = Arc::new(AtomicBool::new(false));
    let place = ready.clone();
    let at = Spot::from_ints(MID.0 + 350, MID.1 + 160);
    let (mut placed, mut sent) = (false, false);
    super::super::harness::scripted_seat(&server, HeroId::PUDGE, move |session| {
        if session.own_hero().is_none() || !place.load(Ordering::SeqCst) {
            return Vec::new();
        }
        let issue = bota_client_core::IssuedOrder::hero;
        if !placed {
            placed = true;
            return vec![
                issue(bota_proto::Order::Cheat {
                    cheat: Cheat::Teleport { at },
                }),
                issue(bota_proto::Order::Move {
                    target: bota_proto::Target::None,
                }),
            ];
        }
        if moving.load(Ordering::SeqCst) && !sent {
            sent = true;
            return vec![issue(bota_proto::Order::MoveDirection {
                toward: at + Spot::from_ints(0, 1000),
            })];
        }
        Vec::new()
    });
    let mut client = player_at_mid(&server, HeroId::SHADOW_FIEND);
    ready.store(true, Ordering::SeqCst);
    client.until("the second seat positioned beside the hero", |c| {
        c.view().units.iter().any(|u| {
            u.kind == UnitKind::Hero && u.owner == Some(bota_proto::SlotId(0)) && u.pos == at
        })
    });
    let enemy = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Hero && u.owner == Some(bota_proto::SlotId(0)))
        .expect("the controlled target")
        .clone();
    assert!(
        !client
            .view()
            .projectiles
            .iter()
            .any(|p| p.source == Some(client.me().id)),
        "Shadow Fiend moving-target scenario no earlier automatic attack contaminates the fixture"
    );
    attack(&mut client, &enemy);
    let (mut began, mut moved) = (false, false);
    for _ in 0..40 {
        client.frames(1);
        moved |= client
            .view()
            .unit(enemy.id)
            .is_some_and(|u| u.pos != enemy.pos);
        if swing(&client).is_some() {
            if began {
                assert_eq!(
                    clip(&client),
                    "attack",
                    "Shadow Fiend moving-target scenario moving target does not re-gate or alternate the current clip"
                );
            }
            began |= clip(&client) == "attack";
            if began {
                go.store(true, Ordering::SeqCst);
            }
        }
        if client
            .view()
            .projectiles
            .iter()
            .any(|p| p.source == Some(client.me().id))
        {
            assert!(
                began && moved,
                "Shadow Fiend moving-target scenario the same windup tracks a target that really moved: began={began} moved={moved} kind={:?}",
                enemy.kind
            );
            return;
        }
    }
    panic!("Shadow Fiend moving-target scenario moving-target attack did not launch");
}
