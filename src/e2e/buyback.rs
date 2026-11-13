//! Buyback as the client shows and gives it: the button on a dead hero's portrait with the cost,
//! a click on it buying back through the server, the stinger and the line of the event, the
//! cooldown and the shortfall on the button after, and the shop button's frame and header.

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_client_core::{CueKind, FeedKind};
use bota_proto::{Cheat, HeroId};

use super::harness::{Client, Server};
use crate::buyback::{BuybackButton, drawn_button, drawn_shop_marks};

fn me(client: &Client) -> &bota_proto::PlayerView {
    let slot = client.core().my_slot.expect("a seat");
    client.view().player(slot).expect("own row")
}

fn gold(client: &Client) -> i32 {
    me(client).gold.expect("own gold")
}

fn state(client: &mut Client) -> BuybackButton {
    drawn_button(client.world_mut()).1
}

/// Clicks the middle of the button as it is drawn.
fn click_button(client: &mut Client) {
    let (button, _, laid_out, _) = drawn_button(client.world_mut());
    assert!(laid_out, "the button is drawn");
    client.hover_ui(button, std::time::Duration::from_secs(20));
    client.click(MouseButton::Left);
}

fn dies(client: &mut Client) {
    client.cheat(Cheat::Suicide);
    client.until("the hero fallen", |c| me(c).respawn_left > 0);
}

/// On the map without a death limit: the hero dies and its button offers the buyback at the
/// cost the server tells; a click buys back, with Dota's stinger and line; dead again, the button
/// shows the cooldown and a click does nothing; with the cooldown reset and no gold it shows the
/// cost in red and a click does nothing; the shop button wears its frame only while the seat has
/// buyback, and its header only while Alt is held.
#[test]
fn a_click_on_a_dead_heros_button_buys_it_back_and_the_button_tells_why_it_cannot() {
    let server = Server::start(1, &["--map", "1"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.until("its buyback told", |c| me(c).buyback.is_some());
    assert_eq!(
        state(&mut client),
        BuybackButton::Hidden,
        "a standing hero has no button"
    );
    client.until("the frame of a seat with buyback", |c| {
        drawn_shop_marks(c.world_mut()).0
    });
    assert!(
        !drawn_shop_marks(client.world_mut()).1,
        "no header without Alt"
    );
    // A hero of level 25 lies dead a hundred seconds: long enough for every look below.
    client.cheat(Cheat::Levels { count: 24 });

    dies(&mut client);
    client.until("the button offering the buyback", |c| {
        matches!(state(c), BuybackButton::Ready { .. })
    });
    let cost = me(&client).buyback.expect("told").cost;
    let before = gold(&client);
    assert_eq!(
        cost,
        200 + before / 13,
        "Dota's cost of what the seat is worth"
    );
    let (_, shown, laid_out, amount) = drawn_button(client.world_mut());
    assert_eq!(shown, BuybackButton::Ready { cost });
    assert!(laid_out);
    assert_eq!(
        amount,
        cost.to_string(),
        "the cost is written on the button"
    );
    let sent = client.record_sent_orders();
    let orders_before = sent.get();
    click_button(&mut client);
    client.until("bought back", |c| me(c).respawn_left == 0);
    assert_eq!(sent.get(), orders_before + 1, "one order for one click");
    assert_eq!(gold(&client), before - cost, "the seat paid the cost");
    assert!(client.core().my_standing_hero().is_some());
    client.until("no button for the risen hero", |c| {
        state(c) == BuybackButton::Hidden
    });
    client.until("the buyer's stinger", |c| {
        crate::sound::cues_sounded(c.world(), CueKind::BoughtBack { own: true }) == 1
    });
    assert_eq!(
        crate::sound::cues_sounded(client.world(), CueKind::BoughtBack { own: false }),
        0,
        "the other players' stinger is not the buyer's"
    );
    let slot = client.core().my_slot.expect("a seat");
    assert!(
        client.core().feed.iter().any(
            |line| matches!(line.kind, FeedKind::BoughtBack { slot: told, .. } if told == slot)
        ),
        "the event's line"
    );
    client.until("the line drawn", |c| {
        c.shown_texts()
            .iter()
            .any(|text| text.to_lowercase().contains("bought back"))
    });
    assert!(
        !drawn_shop_marks(client.world_mut()).0,
        "no frame while the cooldown runs"
    );

    dies(&mut client);
    client.until("the cooldown on the button", |c| {
        matches!(state(c), BuybackButton::Cooling { .. })
    });
    let (_, shown, _, amount) = drawn_button(client.world_mut());
    let BuybackButton::Cooling { seconds } = shown else {
        panic!("{shown:?}");
    };
    assert!(
        (400..=480).contains(&seconds),
        "{seconds} of Dota's 480 left"
    );
    assert_eq!(amount, format!("{}:{:02}", seconds / 60, seconds % 60));
    let orders_before = sent.get();
    click_button(&mut client);
    client.frames(20);
    assert_eq!(
        sent.get(),
        orders_before,
        "a click on a cooling button gives no order"
    );
    assert!(me(&client).respawn_left > 0);

    client.cheat(Cheat::ResetBuyback);
    client.cheat(Cheat::Gold { amount: -100_000 });
    client.until("the shortfall on the button", |c| {
        state(c) == BuybackButton::Short { cost: 200 }
    });
    let orders_before = sent.get();
    click_button(&mut client);
    client.frames(20);
    assert_eq!(
        sent.get(),
        orders_before,
        "a click without the gold gives no order"
    );
    assert!(me(&client).respawn_left > 0);
    client.key(KeyCode::AltLeft, ButtonState::Pressed);
    client.until("the header while Alt is held", |c| {
        drawn_shop_marks(c.world_mut()).1
    });
    let (frame, _, line) = drawn_shop_marks(client.world_mut());
    assert!(!frame, "no frame without the gold");
    assert!(line.contains("-200"), "what is missing: {line}");
    client.key(KeyCode::AltLeft, ButtonState::Released);
    client.until("no header once Alt is let go", |c| {
        !drawn_shop_marks(c.world_mut()).1
    });
}
