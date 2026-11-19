//! The talent window and its keys, as Dota has them: the window opens from the tree button and
//! from the plus plate over it, the tier with a choice open glows, Dota's talent keys (1 for the
//! left talent, 2 for the right one, of the lowest tier with one open) learn only while the window
//! is pinned open and leave control groups and items alone otherwise, and a talent asked for too
//! early is refused with what it waits for.

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_proto::{Cheat, HeroId, ItemId, TalentSlot, Talents};

use super::harness::{Client, Server};
use crate::talents::{Look, Mode, shown};

/// A client at `level` with its hero invulnerable and the talents of that level open.
fn at_level(server: &Server, level: u8, files: &[(&str, &str)]) -> Client {
    let mut client = Client::join_with_files(server, HeroId::SHADOW_FIEND, files);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::Levels { count: level - 1 });
    client.until("the talents of that level open", |c| {
        learnable(c).bits != 0 || level < 10
    });
    client.frames(3);
    client
}

fn told(client: &Client) -> bota_proto::TalentView {
    let core = client.core();
    core.talents_of(core.my_slot.expect("a seat"))
        .expect("the hero's talents")
}

/// The talents one's own hero could take now.
fn learnable(client: &Client) -> Talents {
    told(client).may_learn.unwrap_or_default()
}

fn learned(client: &Client) -> Talents {
    told(client).learned
}

fn slots(slots: &[u8]) -> Talents {
    slots
        .iter()
        .fold(Talents::default(), |set, slot| set.with(TalentSlot(*slot)))
}

fn expected_title(client: &Client, key: &str) -> String {
    client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label(key, &[])
        .expect("the installed window title")
        .to_uppercase()
}

fn level_refusal(client: &Client, level: u8) -> String {
    client
        .world()
        .resource::<crate::dota_text::Texts>()
        .label("dota_hud_error_ability_cant_upgrade_hero_level", &[])
        .expect("the installed level refusal")
        .replace("%s1", &level.to_string())
}

/// Presses and lets go of a key.
fn tap(client: &mut Client, key: KeyCode) {
    client.key(key, ButtonState::Pressed);
    client.key(key, ButtonState::Released);
    client.frames(2);
}

#[test]
fn with_a_free_point_at_level_ten_the_plus_and_the_button_open_the_window_and_the_open_tier_glows()
{
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    let before = shown(client.world_mut());
    assert_eq!(before.mode, Mode::Closed, "the window waits to be asked");
    assert!(
        before.plus,
        "a free point shows the gold plus over the button"
    );
    client.click_named("talent plus");
    let opened = shown(client.world_mut());
    assert_eq!(opened.mode, Mode::Pinned, "the plus pins the window open");
    assert_eq!(
        opened.title,
        expected_title(&client, "DOTA_StatBranch_Title")
    );
    assert_eq!(opened.glowing, vec![0], "only the level-10 tier glows");
    assert_eq!(
        opened.marks,
        [true, true, true],
        "the keys of the left and right talent and of the Attribute Bonus are marked"
    );
    assert_eq!(
        opened.looks[..2],
        [Look::Open, Look::Open],
        "both choices of the open tier are open"
    );
    assert!(
        opened.looks[2..].iter().all(|look| *look == Look::Shut),
        "the tiers not reached are dimmed: {:?}",
        opened.looks
    );
    client.click_named("talent plus");
    assert_eq!(
        shown(client.world_mut()).mode,
        Mode::Closed,
        "a second click lets it go"
    );
    client.click_named("talent button");
    assert_eq!(
        shown(client.world_mut()).mode,
        Mode::Pinned,
        "the button pins it as well"
    );
}

/// With the level-10 tier open, one key learns its talent: the point is spent, the taken side
/// is marked and the other dimmed, and nothing is open to the keys any more.
fn one_key_learns_one_side_of_the_open_tier(key: KeyCode, taken: u8, left_alone: u8) {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    client.click_named("talent button");
    assert_eq!(learnable(&client), slots(&[0, 1]));
    let orders = client.record_sent_orders();
    tap(&mut client, key);
    assert_eq!(orders.get(), 1, "the key sends exactly one talent order");
    client.until("the talent learned", |c| learned(c).has(TalentSlot(taken)));
    client.frames(3);
    assert_eq!(learned(&client), slots(&[taken]), "only the one side");
    assert_eq!(
        learnable(&client),
        Talents::default(),
        "the tier is spent: the other side waits for level 27"
    );
    let window = shown(client.world_mut());
    assert_eq!(
        window.looks[usize::from(taken)],
        Look::Chosen,
        "the chosen side is marked"
    );
    assert_eq!(
        window.looks[usize::from(left_alone)],
        Look::Shut,
        "the other side is dimmed"
    );
    assert!(
        window.glowing.is_empty(),
        "no tier is open: {:?}",
        window.glowing
    );
    assert_eq!(
        window.mode,
        Mode::Pinned,
        "the window stays open for the next choice"
    );
}

#[test]
fn a_pointer_click_learns_the_right_talent_and_leaves_the_tree_open() {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    client.click_named("talent button");
    let orders = client.record_sent_orders();
    client.click_named("talent cell 1");
    assert_eq!(orders.get(), 1, "the click sends exactly one talent order");
    client.until("the right talent was learned", |c| {
        learned(c).has(TalentSlot(1))
    });
    client.step_frames(2);
    let window = shown(client.world_mut());
    assert_eq!(window.looks[..2], [Look::Shut, Look::Chosen]);
    assert_eq!(window.mode, Mode::Pinned);
}

#[test]
fn key_three_spends_a_point_on_the_attribute_bonus() {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    client.click_named("talent button");
    let orders = client.record_sent_orders();
    tap(&mut client, KeyCode::Digit3);
    assert_eq!(orders.get(), 1, "the key sends exactly one attribute order");
    client.until("the Attribute Bonus was learned", |c| {
        told(c).attribute_bonus == 1
    });
    assert_eq!(learned(&client), Talents::default());
}

#[test]
fn key_one_learns_the_left_talent_of_the_open_tier_and_the_right_one_goes_dim() {
    one_key_learns_one_side_of_the_open_tier(KeyCode::Digit1, 0, 1);
}

#[test]
fn key_two_learns_the_right_talent_of_the_open_tier_and_the_left_one_goes_dim() {
    one_key_learns_one_side_of_the_open_tier(KeyCode::Digit2, 1, 0);
}

#[test]
fn at_level_fifteen_a_key_takes_the_lowest_open_tier_first_and_then_the_next() {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 15, &[]);
    client.click_named("talent button");
    assert_eq!(
        learnable(&client),
        slots(&[0, 1, 2, 3]),
        "both tiers are open"
    );
    assert_eq!(
        shown(client.world_mut()).glowing,
        vec![0, 1],
        "both tiers glow"
    );
    tap(&mut client, KeyCode::Digit1);
    client.until("the level-10 talent learned", |c| {
        learned(c).has(TalentSlot(0))
    });
    assert_eq!(
        learned(&client),
        slots(&[0]),
        "the level-10 tier's left talent goes first, as the lowest tier"
    );
    tap(&mut client, KeyCode::Digit1);
    client.until("the level-15 talent learned", |c| {
        learned(c).has(TalentSlot(2))
    });
    assert_eq!(
        learned(&client),
        slots(&[0, 2]),
        "then the next tier's left talent"
    );
    client.frames(3);
    assert!(
        shown(client.world_mut()).glowing.is_empty(),
        "nothing is left to take"
    );
    tap(&mut client, KeyCode::Digit2);
    assert_eq!(
        crate::error_msg::shown(client.world_mut()).as_deref(),
        Some(level_refusal(&client, 27).as_str()),
        "no tier has a right talent open: the refusal says the level the lowest one waits for"
    );
    assert_eq!(learned(&client), slots(&[0, 2]), "nothing more is learned");
}

#[test]
fn a_talent_key_with_the_window_closed_leaves_the_talents_alone_and_picks_the_control_group() {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    client.core_mut().act(bota_client_core::Intent::SelectHero);
    client.key(KeyCode::ControlLeft, ButtonState::Pressed);
    tap(&mut client, KeyCode::Digit1);
    client.key(KeyCode::ControlLeft, ButtonState::Released);
    let hero = client.me().id;
    client
        .core_mut()
        .act(bota_client_core::Intent::SelectUnit(None));
    client.frames(2);
    assert_eq!(client.core().selected, None);
    tap(&mut client, KeyCode::Digit1);
    assert_eq!(
        client.core().selected,
        Some(hero),
        "with the window closed, 1 is the control group's"
    );
    assert_eq!(
        learned(&client),
        Talents::default(),
        "no talent was learned by the control group's key"
    );
    assert_eq!(shown(client.world_mut()).mode, Mode::Closed);
}

#[test]
fn a_talent_key_with_the_window_open_does_not_pick_the_control_group() {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    client.core_mut().act(bota_client_core::Intent::SelectHero);
    client.key(KeyCode::ControlLeft, ButtonState::Pressed);
    tap(&mut client, KeyCode::Digit1);
    client.key(KeyCode::ControlLeft, ButtonState::Released);
    client
        .core_mut()
        .act(bota_client_core::Intent::SelectUnit(None));
    client.click_named("talent button");
    tap(&mut client, KeyCode::Digit1);
    client.until("the talent learned", |c| learned(c).has(TalentSlot(0)));
    assert_eq!(
        client.core().selected,
        None,
        "with the window open, 1 learns the talent and does not pick the group"
    );
}

#[test]
fn an_item_on_a_talent_key_is_taken_up_with_the_window_closed_and_leaves_the_talent_key_to_the_window()
 {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    let item_1 = crate::keys::index_of(crate::keys::Action::Item(0)).expect("bound");
    client
        .world_mut()
        .resource_mut::<crate::config::Settings>()
        .keys[item_1] = crate::keys::Chord::parse("1");
    client.cheat(Cheat::Item {
        item: ItemId::CLARITY,
    });
    client.until("the Clarity carried", |c| {
        c.slot_of(ItemId::CLARITY).is_some()
    });
    let slot = client.slot_of(ItemId::CLARITY).expect("the Clarity's slot");
    assert_eq!(
        slot, 0,
        "the first item is in the first slot, where the key is bound"
    );
    client.core_mut().act(bota_client_core::Intent::SelectHero);
    tap(&mut client, KeyCode::Digit1);
    assert!(
        client.core().aiming.is_some(),
        "with the window closed, 1 takes up the item bound to it for aiming"
    );
    tap(&mut client, KeyCode::Escape);
    assert!(client.core().aiming.is_none(), "Escape lets go of it");
    client.click_named("talent button");
    tap(&mut client, KeyCode::Digit1);
    client.until("the talent learned", |c| learned(c).has(TalentSlot(0)));
    assert!(
        client.core().aiming.is_none(),
        "with the window open, 1 learns the talent and takes up no item"
    );
}

#[test]
fn a_talent_of_a_tier_not_yet_reached_is_refused_with_the_level_it_waits_for() {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    client.click_named("talent button");
    let orders = client.record_sent_orders();
    client.click_named("talent cell 2");
    assert_eq!(
        crate::error_msg::shown(client.world_mut()).as_deref(),
        Some(level_refusal(&client, 15).as_str()),
        "the refusal names the level"
    );
    assert_eq!(orders.get(), 0, "a locked talent sends no order");
    assert_eq!(
        learned(&client),
        Talents::default(),
        "nothing was asked of the server"
    );
}

#[test]
fn a_talent_already_learned_is_refused_as_such() {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    client.click_named("talent button");
    tap(&mut client, KeyCode::Digit1);
    client.until("the talent learned", |c| learned(c).has(TalentSlot(0)));
    client.click_named("talent cell 0");
    assert_eq!(
        crate::error_msg::shown(client.world_mut()).as_deref(),
        Some("That talent is learned already"),
        "the refusal says it is taken"
    );
}

#[test]
fn the_attribute_bonus_asked_for_below_its_level_is_refused_with_the_level_it_needs() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.click_named("talent button");
    let row = {
        let world = client.world_mut();
        world
            .query_filtered::<Entity, With<crate::talents::AttributeRow>>()
            .single(world)
            .expect("the Attribute Bonus row")
    };
    let at = client.centre_of(row).expect("the row is laid out");
    client.mouse(at);
    client.frames(2);
    client.click(MouseButton::Left);
    client.frames(3);
    assert_eq!(
        crate::error_msg::shown(client.world_mut()).as_deref(),
        Some(level_refusal(&client, 2).as_str()),
        "the first level of the bonus waits for level 2"
    );
}

#[test]
fn the_window_hovered_over_the_button_is_a_tooltip_that_learns_nothing() {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    let button = client
        .world_mut()
        .query_filtered::<(Entity, &Name), With<crate::talents::TalentButton>>()
        .iter(client.world())
        .map(|(entity, _)| entity)
        .next()
        .expect("the talent button");
    client.hover_ui(button, std::time::Duration::from_secs(20));
    let tooltip = shown(client.world_mut());
    assert_eq!(
        tooltip.mode,
        Mode::Tooltip,
        "the cursor over the button shows the tree"
    );
    assert_eq!(
        tooltip.title,
        expected_title(&client, "DOTA_StatBranch_TooltipTitle")
    );
    assert!(tooltip.glowing.is_empty(), "no glow in the tooltip");
    assert_eq!(tooltip.marks, [false; 3], "no key marks in the tooltip");
    tap(&mut client, KeyCode::Digit1);
    assert_eq!(
        learned(&client),
        Talents::default(),
        "the talent keys wait for the pinned window"
    );
}

#[test]
fn the_dead_hero_refuses_pointer_and_key_learning_without_sending() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Levels { count: 9 });
    client.until("the level-ten choices open", |c| {
        learnable(c).has(TalentSlot(0))
    });
    client.click_named("talent button");
    client.cheat(Cheat::Suicide);
    client.until("the hero is waiting to respawn", |c| {
        c.view()
            .player(c.core().my_slot.expect("own seat"))
            .is_some_and(|p| p.respawn_left > 0)
    });
    client.hold_incoming();
    let orders = client.record_sent_orders();
    for (button, key) in [
        ("talent cell 0", KeyCode::Digit1),
        ("talent attribute row", KeyCode::Digit3),
    ] {
        client.click_named(button);
        assert_eq!(orders.get(), 0, "{button} sent a learn order while dead");
        assert_eq!(
            crate::error_msg::shown(client.world_mut()).as_deref(),
            Some("You cannot learn while dead")
        );
        client.core_mut().reject = None;
        tap(&mut client, key);
        assert_eq!(orders.get(), 0, "{key:?} sent a learn order while dead");
        assert_eq!(
            crate::error_msg::shown(client.world_mut()).as_deref(),
            Some("You cannot learn while dead")
        );
        client.core_mut().reject = None;
    }
}

#[test]
fn a_second_learning_key_waits_until_the_first_order_is_visible_in_the_view() {
    for key in [KeyCode::Digit1, KeyCode::Digit3] {
        let server = Server::start(1, &[]);
        let mut client = at_level(&server, 15, &[]);
        client.click_named("talent button");
        client.hold_incoming();
        let orders = client.record_sent_orders();
        tap(&mut client, key);
        assert_eq!(orders.get(), 1);
        tap(&mut client, key);
        assert_eq!(orders.get(), 1, "the stale view repeated {key:?}");
        let seq = client
            .core()
            .session()
            .unwrap()
            .ledger()
            .last_seq()
            .unwrap();
        client
            .core_mut()
            .handle(bota_proto::ServerMsg::OrderRejected {
                seq,
                reason: bota_proto::RejectReason::CannotLearn,
            });
        tap(&mut client, key);
        assert_eq!(orders.get(), 2, "a rejected order can be retried");
    }
}

fn drawn_rect<T: Component>(client: &mut Client) -> Rect {
    let world = client.world_mut();
    let (node, transform) = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<T>>()
        .single(world)
        .expect("one laid-out panel");
    Rect::from_center_size(transform.translation, node.size())
}

fn assert_inside_window(client: &mut Client) {
    let panel = drawn_rect::<crate::talents::TalentPanel>(client);
    let world = client.world_mut();
    let window = world.query::<&Window>().single(world).expect("a window");
    assert!(
        panel.min.min_element() >= -1.0,
        "{panel:?} starts outside the window"
    );
    assert!(
        panel.max.x <= window.physical_width() as f32 + 1.0,
        "{panel:?} passes the right edge"
    );
    assert!(
        panel.max.y <= window.physical_height() as f32 + 1.0,
        "{panel:?} passes the bottom edge"
    );
}

#[test]
fn the_talent_window_fits_at_1024_by_600() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--window", "1024x600"]);
    client.click_named("talent button");
    assert_inside_window(&mut client);
    assert!(crate::talents::overflowing(client.world_mut()).is_empty());
}

#[test]
fn the_talent_window_fits_at_double_interface_scale() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--set", "ui_scale=2"]);
    client.click_named("talent button");
    assert_inside_window(&mut client);
    assert!(crate::talents::overflowing(client.world_mut()).is_empty());
}

#[test]
fn the_open_shop_leaves_every_talent_choice_accessible_at_1024_by_600() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(
        &server,
        HeroId::SHADOW_FIEND,
        &["--window", "1024x600", "--set", "ui_scale=2"],
    );
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .shop = true;
    client.click_named("talent button");
    assert_inside_window(&mut client);
    client.hover_ui_with(std::time::Duration::from_secs(20), |c| {
        let world = c.world_mut();
        world
            .query_filtered::<(Entity, &Name), With<bevy::ui_widgets::Button>>()
            .iter(world)
            .find_map(|(entity, name)| (name.as_str() == "talent cell 7").then_some(entity))
    });
    let talent = drawn_rect::<crate::talents::TalentPanel>(&mut client);
    let shop = drawn_rect::<crate::shop::ShopPanel>(&mut client);
    assert!(
        talent.max.x <= shop.min.x + 1.0,
        "{talent:?} overlaps {shop:?}"
    );
}

#[test]
fn the_pinned_window_aligns_with_the_hud_and_clears_the_attribute_badge() {
    let server = Server::start(1, &[]);
    let mut client = at_level(&server, 10, &[]);
    client.click_named("talent button");
    let panel = drawn_rect::<crate::talents::TalentPanel>(&mut client);
    let attribute = drawn_rect::<crate::talents::AttributeRow>(&mut client);
    let world = client.world_mut();
    let rect = |world: &World, entity| {
        let node = world.get::<ComputedNode>(entity).expect("node laid out");
        let transform = world.get::<UiGlobalTransform>(entity).expect("node place");
        Rect::from_center_size(transform.translation, node.size())
    };
    let hud = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find_map(|(entity, name)| (name.as_str() == "hud panel").then_some(entity))
        .expect("HUD panel");
    let mark = world
        .query::<(Entity, &crate::keys::KeyLabel, &ChildOf)>()
        .iter(world)
        .find_map(|(_, label, parent)| {
            (label.0 == crate::keys::Action::TalentAttribute).then_some(parent.parent())
        })
        .expect("attribute key mark");
    assert!(
        (panel.min.x - rect(world, hud).min.x).abs() <= 2.0,
        "{panel:?} does not align with the HUD"
    );
    assert!(
        rect(world, mark).max.y <= attribute.min.y,
        "the key mark overlaps the attribute plate"
    );
    let scale = world.resource::<UiScale>().0;
    assert!(
        rect(world, hud).min.y - panel.max.y >= 50.0 * scale,
        "the window must clear the strip over the HUD"
    );
}
