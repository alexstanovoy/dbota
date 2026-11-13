//! Local profile and catalogue navigation remain usable without a service account.
use super::menu::{act, local_menu};
use crate::main_menu::{Act, MenuCore, Page};
use bevy::ui_widgets::Button as NativeButton;

#[test]
fn the_local_player_can_open_their_profile_without_a_service_account() {
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::Profile));
    assert_eq!(client.world().non_send::<MenuCore>().page(), Page::Profile);
}

#[test]
fn the_heroes_tab_opens_the_catalogue_and_a_hero_page_without_an_account() {
    use bevy::prelude::*;
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::LocalLobby));
    let world = client.world_mut();
    let button = world
        .query_filtered::<(Entity, &Name), With<NativeButton>>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "heroes")
        .map(|(entity, _)| entity)
        .expect("HEROES is available without an account");
    let at = client
        .centre_of(button)
        .expect("the Heroes tab is laid out");
    client.mouse(at);
    client.click(MouseButton::Left);
    client.frames(5);
    let world = client.world_mut();
    let hero = world
        .query_filtered::<(Entity, &Name), With<NativeButton>>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "Shadow Fiend")
        .map(|(entity, _)| entity)
        .expect("the playable hero is in the catalogue");
    let at = client.centre_of(hero).expect("the hero card is laid out");
    client.mouse(at);
    client.click(MouseButton::Left);
    client.frames(5);
    // The innate is the round mark among the hero's icons (it names no text: see e2e::hero_page).
    let world = client.world_mut();
    let innate = world
        .query::<(&Name, &InheritedVisibility)>()
        .iter(world)
        .any(|(name, shown)| shown.get() && name.as_str() == "ability necromastery");
    assert!(innate, "the hero page includes its innate");
}

#[test]
fn the_profile_shows_the_local_players_record_from_a_real_match_metadata_file() {
    use bevy::prelude::*;
    let (replay, meta) = crate::postgame::recorded_match_for_menu();
    let directory = fresh_library("menu_profile");
    std::fs::create_dir_all(&directory).expect("private fixture folder");
    let copied = directory.join("match.brp");
    let bytes = std::fs::copy(replay, &copied).expect("copy real match");
    std::fs::copy(meta, copied.with_extension("meta")).expect("copy real metadata");
    std::fs::write(
        directory.join("index.txt"),
        format!("dbota-replays 1\nr\t100\t{bytes}\t-\tlocal\tmatch.brp\t\t\t0\n"),
    )
    .expect("record the local slot explicitly");
    let mut client = local_menu();
    crate::main_menu::tests::install_local_library(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
        &directory,
    );
    act(&mut client, Act::Go(Page::Replays));
    client.until("the private library was read", |client| {
        crate::main_menu::tests::library_titles_ready(client.world().non_send::<MenuCore>())
    });
    act(&mut client, Act::Go(Page::Profile));
    client.within(
        std::time::Duration::from_secs(15),
        "the owned recording is drawn",
        |client| {
            crate::main_menu::tests::page_words(client.world_mut())
                .iter()
                .any(|word| word == "Shadow Fiend")
        },
    );
    assert!(
        crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|text| text == "Shadow Fiend"),
        "Profile shows the hero from the owned local metadata"
    );
    assert_eq!(
        client.world().non_send::<MenuCore>().last_played_hero(),
        Some(bota_proto::HeroId::SHADOW_FIEND)
    );
}

#[test]
fn the_profile_avatar_editor_is_honest_when_the_service_has_no_avatar_backend() {
    use bevy::prelude::*;
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::Profile));
    let world = client.world_mut();
    assert!(
        !world
            .query_filtered::<&Name, With<NativeButton>>()
            .iter(world)
            .any(|name| name.as_str() == "edit avatar"),
        "no pencil offers an unavailable feature"
    );
    act(
        &mut client,
        Act::Avatar(crate::main_menu::AvatarAction::Open),
    );
    assert!(
        !crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|word| word == "CHANGE AVATAR")
    );
    act(&mut client, Act::Go(Page::Heroes));
    assert_eq!(
        client.world().non_send::<MenuCore>().page(),
        Page::Heroes,
        "no invisible modal blocks navigation"
    );
}

#[test]
fn local_profile_details_survive_a_restart_after_the_replay_is_pruned() {
    use bevy::prelude::*;
    let (replay, meta) = crate::postgame::recorded_match_for_menu();
    let directory = fresh_library("menu_profile_pruned");
    std::fs::create_dir_all(&directory).unwrap();
    let copied = directory.join("retained.brp");
    let bytes = std::fs::copy(replay, &copied).unwrap();
    std::fs::copy(meta, copied.with_extension("meta")).unwrap();
    std::fs::write(
        directory.join("index.txt"),
        format!("dbota-replays 1\nr\t100\t{bytes}\t-\tlocal\tretained.brp\t\t\t0\n"),
    )
    .unwrap();
    {
        let mut client = local_menu();
        crate::main_menu::tests::install_local_library(
            &mut client.world_mut().non_send_mut::<MenuCore>(),
            &directory,
        );
        act(&mut client, Act::Go(Page::Profile));
        client.until("owned metadata is visible", |client| {
            crate::main_menu::tests::page_words(client.world_mut())
                .iter()
                .any(|text| text == "Shadow Fiend")
        });
        assert!(directory.join("matches.json").is_file());
    }
    std::fs::remove_file(&copied).unwrap();
    std::fs::remove_file(copied.with_extension("meta")).unwrap();
    std::fs::write(directory.join("index.txt"), "dbota-replays 1\n").unwrap();
    let mut client = local_menu();
    crate::main_menu::tests::install_local_library(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
        &directory,
    );
    act(&mut client, Act::Go(Page::Profile));
    assert!(
        crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|text| text == "Shadow Fiend")
    );
    let world = client.world_mut();
    assert!(
        world
            .query_filtered::<&Name, With<crate::main_menu::Disabled>>()
            .iter(world)
            .any(|name| name.as_str() == "watch")
    );
    act(&mut client, Act::MatchDetails("retained.brp".to_owned()));
    assert!(
        client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible(),
        "retained details open without replay bytes"
    );
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "RADIANT VICTORY"),
        "the drawn result retains the independently expected outcome"
    );
    client.click_named("results scoreboard");
    assert!(
        client
            .shown_texts_within::<crate::postgame::render::SeatRow>()
            .iter()
            .any(|text| text == "Shadow Fiend"),
        "the retained scoreboard rows still identify the recorded hero after restart and pruning"
    );
    assert!(client.shown_texts().iter().any(|text| text == "NET WORTH"));
    client.click_named("results close");
    assert!(
        !client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible()
    );
    assert_eq!(client.world().non_send::<MenuCore>().page(), Page::Profile);
}

#[test]
fn leaderboard_uses_the_service_filters_and_profile_hides_unplayed_ratings() {
    let service = crate::main_menu::tests::service();
    let mut client = super::menu::signed_up(&service, "ladder_mira");
    act(&mut client, Act::Go(Page::Leaderboard));
    client.within(
        std::time::Duration::from_secs(15),
        "the SDK leaderboard reply was drawn",
        |client| {
            crate::main_menu::tests::page_words(client.world_mut())
                .iter()
                .any(|text| text == "No matching players.")
        },
    );
    let words = crate::main_menu::tests::page_words(client.world_mut());
    assert!(words.iter().any(|text| text == "0–0 of 0"));
    act(
        &mut client,
        Act::LeaderboardPlayers(crate::main_menu::LeaderboardPlayers::Bots),
    );
    client.within(
        std::time::Duration::from_secs(15),
        "the Bots reply was drawn",
        |client| {
            crate::main_menu::tests::page_words(client.world_mut())
                .iter()
                .any(|text| text == "No matching players.")
        },
    );
    assert!(
        !crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|text| text.starts_with("ladder_mira#"))
    );
    act(&mut client, Act::Go(Page::Profile));
    client.frames(5);
    let words = crate::main_menu::tests::page_words(client.world_mut());
    assert!(!words.iter().any(|text| text == "RATINGS · PER MODE"));
    assert!(!words.iter().any(|text| text == "UNRANKED"));
}

#[test]
fn avatar_picker_crop_and_save_publish_only_a_validated_backend_reply() {
    use crate::main_menu::{AVATAR_TEST_PNG, AvatarAction, AvatarRequest};
    let service = crate::main_menu::tests::service();
    let mut client = super::menu::signed_up(&service, "avatar_mira");
    act(&mut client, Act::Go(Page::Profile));
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .avatar_backend(true);
    act(&mut client, Act::Avatar(AvatarAction::Open));
    act(&mut client, Act::Avatar(AvatarAction::Upload));
    let Some(AvatarRequest::Pick { token }) = client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .take_avatar_request()
    else {
        panic!("picker requested only with a backend");
    };
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .avatar_selected(token, Some(AVATAR_TEST_PNG.to_vec()));
    client.until("the bounded image decode finished", |client| {
        crate::main_menu::tests::avatar_crop_ready(client.world().non_send::<MenuCore>())
    });
    client.frames(5);
    act(&mut client, Act::Avatar(AvatarAction::Save));
    let Some(AvatarRequest::Save {
        token,
        crop: Some(crop),
    }) = client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .take_avatar_request()
    else {
        panic!("a crop is ready for the SDK adapter");
    };
    assert_eq!((crop.width(), crop.height()), (128, 128));
    assert!(
        crop.rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == [20, 90, 180, 255])
    );
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .avatar_saved(token, Ok(Some(AVATAR_TEST_PNG.to_vec())));
    client.frames(5);
    assert!(
        !crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|text| text == "CHANGE AVATAR")
    );
    act(&mut client, Act::Avatar(AvatarAction::Open));
    assert!(
        crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .all(|text| !text.contains("Preview of the")),
        "saved image is used on the next open"
    );
}

#[test]
fn the_hero_pages_demo_button_opens_the_chosen_hero_in_a_local_sandbox() {
    use bevy::prelude::*;
    let mut client = local_menu();
    crate::main_menu::tests::use_matching_local_server(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
    );
    act(&mut client, Act::InspectHero(bota_proto::HeroId::PUDGE));
    let world = client.world_mut();
    let button = world
        .query_filtered::<(Entity, &Name), With<NativeButton>>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "demo hero")
        .map(|(entity, _)| entity)
        .unwrap();
    let at = client.centre_of(button).unwrap();
    client.mouse(at);
    client.click(MouseButton::Left);
    client.until("the chosen hero stands in the sandbox", |client| {
        client.core().my_hero().is_some()
    });
    assert_eq!(client.me().hero, Some(bota_proto::HeroId::PUDGE));
    assert!(
        client
            .core()
            .statics()
            .is_some_and(|statics| statics.metadata().cheats)
    );
    assert_eq!(
        client.world().non_send::<MenuCore>().demo_hero(),
        Some(bota_proto::HeroId::PUDGE)
    );
    assert!(
        client
            .world()
            .non_send::<MenuCore>()
            .last_played_hero()
            .is_none(),
        "a sandbox is not a locally recorded match"
    );
}

#[test]
fn the_avatar_dialog_blocks_clicks_on_navigation_beneath_its_scrim() {
    use bevy::prelude::*;
    let service = crate::main_menu::tests::service();
    let mut client = super::menu::signed_up(&service, "modal_mira");
    act(&mut client, Act::Go(Page::Profile));
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .avatar_backend(true);
    act(
        &mut client,
        Act::Avatar(crate::main_menu::AvatarAction::Open),
    );
    let world = client.world_mut();
    let heroes = world
        .query_filtered::<(Entity, &Name), With<NativeButton>>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "heroes")
        .map(|(entity, _)| entity)
        .unwrap();
    let at = client.centre_of(heroes).unwrap();
    client.mouse(at);
    client.click(MouseButton::Left);
    client.frames(5);
    assert_eq!(client.world().non_send::<MenuCore>().page(), Page::Profile);
}

#[test]
fn homes_background_uses_the_local_replays_owned_team_without_a_service() {
    use bevy::prelude::*;
    let (replay, meta) = crate::postgame::recorded_match_for_menu();
    let directory = fresh_library("menu_dire_side");
    std::fs::create_dir_all(&directory).unwrap();
    let copied = directory.join("dire.brp");
    let bytes = std::fs::copy(replay, &copied).unwrap();
    let mut meta = bota_replay::meta::decode(&std::fs::read(meta).unwrap()).unwrap();
    // This fixture has one occupied seat. Control its recorded team to isolate the menu's
    // source selection; simulation team assignment is covered by the game server's tests.
    for record in &mut meta.records {
        if let bota_replay::meta::MetaRecord::Seat { slot: 0, team, .. } = record {
            *team = 1;
        }
    }
    let mut writer = bota_replay::meta::Writer::default();
    assert!(writer.push(&bota_replay::meta::MetaRecord::Header(meta.header), false));
    for record in meta.records {
        assert!(writer.push(&record, true));
    }
    std::fs::write(copied.with_extension("meta"), writer.into_bytes()).unwrap();
    std::fs::write(
        directory.join("index.txt"),
        format!("dbota-replays 1\nr\t100\t{bytes}\t-\tlocal\tdire.brp\t\t\t0\n"),
    )
    .unwrap();
    let mut client = local_menu();
    crate::main_menu::tests::install_local_library(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
        &directory,
    );
    act(&mut client, Act::Go(Page::Profile));
    client.within(
        std::time::Duration::from_secs(15),
        "local metadata is visible",
        |client| {
            crate::main_menu::tests::page_words(client.world_mut())
                .iter()
                .any(|text| text == "Shadow Fiend")
        },
    );
    act(&mut client, Act::Go(Page::Home));
    let world = client.world_mut();
    let images: Vec<_> = world
        .query::<(&ImageNode, &InheritedVisibility)>()
        .iter(world)
        .filter(|(_, visibility)| visibility.get())
        .map(|(image, _)| image.image.clone())
        .collect();
    let server = world.resource::<AssetServer>();
    assert!(
        images.iter().any(|image| server
            .get_path(image.id())
            .is_some_and(|path| path.path().ends_with("menu/direancient_loadingscreen.png"))),
        "Home draws the locally recorded Dire background"
    );
}

#[test]
fn signing_out_removes_the_service_identity_from_the_local_profile() {
    let service = crate::main_menu::tests::service();
    let mut client = super::menu::signed_up(&service, "previous_player");
    act(&mut client, Act::Go(Page::Profile));
    client.within(
        std::time::Duration::from_secs(15),
        "the signed-in profile",
        |client| {
            crate::main_menu::tests::page_words(client.world_mut())
                .iter()
                .any(|word| word.starts_with("previous_player#"))
        },
    );
    act(&mut client, Act::Logout);
    act(&mut client, Act::Go(Page::Profile));
    let words = crate::main_menu::tests::page_words(client.world_mut());
    assert!(
        !words
            .iter()
            .any(|word| word.starts_with("previous_player#")),
        "signed-out Profile uses the local name"
    );
}

#[test]
fn catalogue_portraits_cover_their_cards_without_stretching_or_letterboxing() {
    use bevy::prelude::*;
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::Heroes));
    let world = client.world_mut();
    let cards: Vec<_> = world
        .query_filtered::<(Entity, &Name), With<NativeButton>>()
        .iter(world)
        .filter(|(_, name)| matches!(name.as_str(), "Pudge" | "Shadow Fiend"))
        .map(|(entity, _)| entity)
        .collect();
    assert_eq!(cards.len(), 2);
    let pictures: Vec<_> = world
        .query::<(Entity, &ImageNode)>()
        .iter(world)
        .filter_map(|(entity, _)| {
            let mut parent = entity;
            for _ in 0..16 {
                parent = world.get::<ChildOf>(parent)?.parent();
                if cards.contains(&parent) {
                    return Some((entity, parent));
                }
            }
            None
        })
        .collect();
    assert_eq!(pictures.len(), 2);
    client.within(
        std::time::Duration::from_secs(15),
        "catalogue portraits loaded",
        |client| {
            let world = client.world();
            pictures.iter().all(|(image, _)| {
                world.get::<ImageNode>(*image).is_some_and(|node| {
                    world.resource::<Assets<Image>>().get(&node.image).is_some()
                })
            })
        },
    );
    client.frames(3);
    for (image, card) in pictures {
        let world = client.world();
        let card_size = world.get::<ComputedNode>(card).unwrap().size();
        let image_size = world.get::<ComputedNode>(image).unwrap().size();
        assert!(
            image_size.y + 1.0 >= card_size.y && image_size.x + 1.0 >= card_size.x,
            "the picture fills its whole card"
        );
        let picture = world.get::<ImageNode>(image).unwrap();
        let source = world
            .resource::<Assets<Image>>()
            .get(&picture.image)
            .unwrap()
            .size_f32();
        let crop = picture
            .rect
            .unwrap_or(Rect::from_corners(Vec2::ZERO, source));
        assert!(
            (crop.width() / crop.height() - card_size.x / card_size.y).abs() < 0.01,
            "the drawn source is cropped to the frame shape, preserving the portrait's proportions"
        );
    }
}

fn design_preview(name: &str) -> super::harness::Client {
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::Heroes));
    client
        .world_mut()
        .insert_resource(crate::main_menu::Preview(
            crate::main_menu::preview(name).unwrap(),
        ));
    client.frames(8);
    client
}

#[test]
fn a_profile_with_no_rated_games_omits_the_whole_ratings_section() {
    use crate::main_menu::{PageView, Preview};
    let mut client = design_preview("profile");
    let mut view = crate::main_menu::preview("profile").unwrap();
    let PageView::Profile { ratings, .. } = &mut view.body else {
        panic!("profile");
    };
    for rating in ratings.iter_mut() {
        rating.games = 0;
    }
    client.world_mut().insert_resource(Preview(view.clone()));
    client.frames(5);
    assert!(
        !crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|word| word == "RATINGS · PER MODE")
    );
    let PageView::Profile { ratings, .. } = &mut view.body else {
        unreachable!()
    };
    ratings.clear();
    client.world_mut().insert_resource(Preview(view));
    client.frames(5);
    assert!(
        !crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|word| word == "RATINGS · PER MODE")
    );
}

#[test]
fn unavailable_leaderboard_has_only_its_actionable_message() {
    let mut client = design_preview("leaderboard-unavailable");
    let mut view = crate::main_menu::preview("leaderboard-unavailable").unwrap();
    let crate::main_menu::PageView::Leaderboard { board, .. } = &mut view.body else {
        panic!("board");
    };
    board.notice = Some("stale service detail".to_owned());
    client
        .world_mut()
        .insert_resource(crate::main_menu::Preview(view));
    client.frames(5);
    let words = crate::main_menu::tests::page_words(client.world_mut());
    assert_eq!(
        words
            .iter()
            .filter(|word| word.as_str() == "Leaderboard unavailable. Try again later.")
            .count(),
        1
    );
    assert!(!words.iter().any(|word| word == "Up to 100 players"
        || word == "stale service detail"
        || word.contains(" of ")));
}

#[test]
fn leaderboard_footer_describes_loaded_rows_and_changes_with_the_visible_range() {
    use crate::main_menu::{PageView, Preview};
    use bevy::prelude::*;
    let mut client = design_preview("leaderboard");
    let mut view = crate::main_menu::preview("leaderboard").unwrap();
    let PageView::Leaderboard { board, .. } = &mut view.body else {
        panic!("board");
    };
    let row = board.rows[0].clone();
    board.rows = (1..=100)
        .map(|rank| {
            let mut row = row.clone();
            row.rank = rank;
            row.account = u64::from(rank);
            row.nickname = format!("player{rank}");
            row
        })
        .collect();
    board.own = None;
    board.total = 300;
    client.world_mut().insert_resource(Preview(view.clone()));
    client.frames(8);
    let footer = |client: &mut super::harness::Client| {
        crate::main_menu::tests::page_words(client.world_mut())
            .into_iter()
            .find(|word| word.ends_with(" of 100"))
            .expect("range footer for 100 loaded rows")
    };
    let first = footer(&mut client);
    assert_eq!(first, "1–16 of 100");
    let window = client
        .world_mut()
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(client.world())
        .unwrap();
    client
        .world_mut()
        .write_message(bevy::input::mouse::MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Line,
            x: 0.0,
            y: -5.0,
            window,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    client.frames(5);
    let scrolled = footer(&mut client);
    assert_ne!(first, scrolled, "scroll changes the visible range");
    let PageView::Leaderboard { board, .. } = &mut view.body else {
        unreachable!()
    };
    board.rows.truncate(3);
    client.world_mut().insert_resource(Preview(view));
    client.frames(8);
    assert!(
        crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|word| word == "1–3 of 3")
    );
}

#[test]
fn last_match_has_a_result_coloured_tint_without_a_glow() {
    use crate::main_menu::{PageView, Preview};
    use bevy::prelude::*;
    for (won, label) in [(true, "VICTORY"), (false, "DEFEAT")] {
        let mut client = design_preview("home");
        let mut view = crate::main_menu::preview("home").unwrap();
        let PageView::Home { matches, .. } = &mut view.body else {
            panic!("home");
        };
        if !won {
            std::sync::Arc::make_mut(matches)[0].result = crate::main_menu::tests::lost_result();
        }
        client.world_mut().insert_resource(Preview(view));
        client.frames(5);
        let world = client.world_mut();
        let title = world
            .query::<(Entity, &Text)>()
            .iter(world)
            .find(|(_, word)| word.0 == label)
            .map(|(entity, _)| entity)
            .unwrap();
        let column = world.get::<ChildOf>(title).unwrap().parent();
        let row = world.get::<ChildOf>(column).unwrap().parent();
        let tint = world
            .get::<BackgroundGradient>(row)
            .expect("result tint behind the hero and result text");
        let Gradient::Linear(gradient) = &tint.0[0] else {
            panic!("horizontal tint");
        };
        let left = gradient.stops.first().unwrap().color.to_srgba();
        let right = gradient.stops.last().unwrap().color.to_srgba();
        assert!(left.alpha > 0.0 && left.alpha < 0.5);
        assert_eq!(right.alpha, 0.0);
        assert_eq!(left.green > left.red, won);
        assert!(world.get::<BoxShadow>(row).is_none());
    }
}

#[test]
fn home_without_a_played_game_draws_no_hero_and_keeps_its_empty_cards() {
    use bevy::prelude::*;
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::Home));
    let world = client.world_mut();
    assert!(
        !world
            .query::<(&Name, &InheritedVisibility)>()
            .iter(world)
            .any(|(name, shown)| name.as_str() == "home hero" && shown.get()),
        "Home has no fallback hero before a locally played game"
    );
    assert!(
        client
            .world()
            .non_send::<MenuCore>()
            .last_played_hero()
            .is_none()
    );
    let words = crate::main_menu::tests::page_words(client.world_mut());
    assert!(words.iter().any(|word| word == "LAST MATCH"));
    assert!(
        words
            .iter()
            .any(|word| word == "No matches on this machine yet.")
    );
}

fn fresh_library(name: &str) -> std::path::PathBuf {
    let directory = crate::test_support::scratch_path(&format!("menu_pages/{name}"));
    if directory.exists() {
        std::fs::remove_dir_all(&directory).expect("clear the previous fixture, including history");
    }
    std::fs::create_dir_all(&directory).expect("fresh library");
    directory
}

fn two_seat_library(name: &str, finished: bool) -> std::path::PathBuf {
    use bota_replay::meta::MetaRecord;
    let (replay, source) = crate::postgame::recorded_match_for_menu();
    let directory = fresh_library(name);
    let bytes = std::fs::copy(replay, directory.join("owned.brp")).unwrap();
    let meta = bota_replay::meta::decode(&std::fs::read(source).unwrap()).unwrap();
    let mut writer = bota_replay::meta::Writer::default();
    assert!(writer.push(&MetaRecord::Header(meta.header), false));
    let mut meta = meta;
    if !finished {
        for record in &mut meta.records {
            if let MetaRecord::Sample { stats, .. } = record {
                stats.retain(|(id, _)| *id != bota_replay::meta::stat::GOLD_EARNED);
            }
        }
    }
    for record in meta.records {
        let other = match &record {
            MetaRecord::Seat { team, .. } => Some(MetaRecord::Seat {
                slot: 1,
                team: *team,
                hero: bota_proto::HeroId::SHADOW_FIEND.0,
            }),
            MetaRecord::Sample { tick, stats, .. } => Some(MetaRecord::Sample {
                tick: *tick,
                slot: 1,
                stats: stats.clone(),
            }),
            MetaRecord::Inventory { tick, items, .. } => Some(MetaRecord::Inventory {
                tick: *tick,
                slot: 1,
                items: items.clone(),
            }),
            _ => None,
        };
        let record = match record {
            MetaRecord::Seat { slot, team, .. } => MetaRecord::Seat {
                slot,
                team,
                hero: bota_proto::HeroId::PUDGE.0,
            },
            MetaRecord::End { .. } if !finished => continue,
            other => other,
        };
        assert!(writer.push(&record, true));
        if let Some(other) = other {
            assert!(writer.push(&other, true));
        }
    }
    std::fs::write(directory.join("owned.meta"), writer.into_bytes()).unwrap();
    std::fs::write(
        directory.join("index.txt"),
        format!("dbota-replays 1\nr\t100\t{bytes}\t-\tlocal\towned.brp\t\t\t1\n"),
    )
    .unwrap();
    directory
}

#[test]
fn a_nonzero_owner_supplies_the_profile_and_home_hero() {
    let directory = two_seat_library("nonzero_owner", true);
    let mut client = local_menu();
    crate::main_menu::tests::install_local_library(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
        &directory,
    );
    act(&mut client, Act::Go(Page::Profile));
    client.within(
        std::time::Duration::from_secs(15),
        "nonzero owner's history is read",
        |client| {
            crate::main_menu::tests::page_words(client.world_mut())
                .iter()
                .any(|word| word == "Shadow Fiend")
        },
    );
    assert!(
        !crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|word| word == "Pudge")
    );
    assert_eq!(
        client.world().non_send::<MenuCore>().last_played_hero(),
        Some(bota_proto::HeroId::SHADOW_FIEND)
    );
}

#[test]
fn unfinished_match_details_only_draw_the_owner_while_finished_details_draw_both_seats() {
    for finished in [false, true] {
        let directory = two_seat_library(
            if finished {
                "finished_details"
            } else {
                "unfinished_details"
            },
            finished,
        );
        // Pin graph facts independently of the curve builder: both recorded seats are Radiant.
        let file = directory.join("owned.meta");
        let mut metadata = bota_replay::meta::decode(&std::fs::read(&file).expect("fixture bytes"))
            .expect("fixture");
        for record in &mut metadata.records {
            if let bota_replay::meta::MetaRecord::Sample { slot, stats, .. } = record {
                for (id, value) in stats {
                    if *id == bota_replay::meta::stat::NET_WORTH {
                        *value = if *slot == 0 { 1500 } else { 500 };
                    }
                    if *id == bota_replay::meta::stat::XP {
                        *value = if *slot == 0 { 900 } else { 300 };
                    }
                }
            }
        }
        let mut writer = bota_replay::meta::Writer::default();
        assert!(writer.push(
            &bota_replay::meta::MetaRecord::Header(metadata.header),
            false
        ));
        for record in metadata.records {
            assert!(writer.push(&record, true));
        }
        std::fs::write(&file, writer.into_bytes()).expect("pinned fixture");
        let mut client = local_menu();
        crate::main_menu::tests::install_local_library(
            &mut client.world_mut().non_send_mut::<MenuCore>(),
            &directory,
        );
        act(&mut client, Act::Go(Page::Profile));
        client.until("metadata is loaded", |client| {
            client
                .world()
                .non_send::<MenuCore>()
                .last_played_hero()
                .is_some()
        });
        act(&mut client, Act::MatchDetails("owned.brp".to_owned()));
        client.click_named("results scoreboard");
        let world = client.world_mut();
        let rows: Vec<_> = world
            .query::<&crate::postgame::render::SeatRow>()
            .iter(world)
            .map(|row| row.0.0)
            .collect();
        assert!(rows.contains(&1), "the owner's drawn row remains: {rows:?}");
        assert_eq!(
            rows.contains(&0),
            finished,
            "another drawn seat requires End: {rows:?}"
        );
        if !finished {
            let world = client.world_mut();
            let rate = world
                .query::<(&crate::postgame::render::Cell, &bevy::prelude::Text)>()
                .iter(world)
                .find(|(cell, _)| **cell == crate::postgame::render::Cell::Gpm)
                .map(|(_, text)| text.0.as_str())
                .expect("owner's GPM cell");
            assert_eq!(
                rate, "—",
                "missing earned gold never becomes a net-worth estimate"
            );
        }
        let world = client.world_mut();
        let graphs = world
            .query::<&bevy::prelude::Name>()
            .iter(world)
            .any(|name| name.as_str() == "results graphs");
        assert_eq!(graphs, finished, "team graphs require End");
        assert!(
            !client
                .shown_texts()
                .iter()
                .any(|text| text == "Team advantage"),
            "a scoreboard never leaks a hidden team graph"
        );
        if finished {
            client.click_named("results graphs");
            assert!(
                client
                    .shown_texts()
                    .iter()
                    .any(|text| text == "Team advantage")
            );
            let shown = client.shown_texts();
            assert!(
                shown.iter().any(|text| text == "+2.0k"),
                "1500 + 500 recorded net worth is visibly graphed"
            );
            assert!(
                shown.iter().any(|text| text == "+1.2k"),
                "900 + 300 recorded experience is visibly graphed"
            );
        }
    }
}

#[test]
fn every_hero_page_draws_pinned_game_values() {
    use bevy::prelude::*;
    use bota_proto::HeroId;
    let expected = [
        (
            HeroId::PUDGE,
            [
                "25", "13", "16", "+ 3.0", "+ 1.4", "+ 1.8", "70–76", "2.2", "175", "280", "1.5 s",
                "670", "267",
            ],
        ),
        (
            HeroId::SHADOW_FIEND,
            [
                "19", "25", "16", "+ 2.7", "+ 3.6", "+ 2.2", "41–47", "4.2", "525", "305", "1.3 s",
                "538", "267",
            ],
        ),
    ];
    assert_eq!(
        expected.len(),
        bota_core::HEROES.len(),
        "add pins when a hero joins the catalogue"
    );
    let mut client = local_menu();
    for (hero, values) in expected {
        act(&mut client, Act::InspectHero(hero));
        let mut words = crate::main_menu::tests::page_words(client.world_mut());
        // The gains are set with their values in one text, as spans of it.
        let world = client.world_mut();
        words.extend(
            world
                .query::<&TextSpan>()
                .iter(world)
                .map(|span| span.0.trim().to_owned()),
        );
        for value in values {
            assert!(
                words.iter().any(|word| word == value),
                "{hero:?} must draw {value:?}: {words:?}"
            );
        }
    }
}

#[test]
fn turning_replay_saving_off_keeps_local_match_history_without_a_replay_file() {
    let (replay, metadata) = crate::postgame::recorded_match_for_menu();
    let directory = fresh_library("saving_off_history");
    let mut bytes = std::fs::read(replay).unwrap();
    bytes.extend(bota_replay::trailer::frame(&std::fs::read(metadata).unwrap()).unwrap());
    let mut client = local_menu();
    {
        let mut core = client.world_mut().non_send_mut::<MenuCore>();
        crate::main_menu::tests::install_local_library(&mut core, &directory);
        core.set_replay_policy(crate::replays::Policy {
            keep: false,
            ..Default::default()
        });
        crate::main_menu::deliver_replay_for_menu(&mut core, &bytes);
    }
    act(&mut client, Act::Go(Page::Profile));
    use bevy::prelude::*;
    let world = client.world_mut();
    let rows: Vec<_> = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .filter(|(_, name)| name.as_str().starts_with("profile match "))
        .map(|(entity, _)| entity)
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "the retained match is drawn without a replay file"
    );
    let hero_column = world
        .get::<Children>(rows[0])
        .expect("match columns")
        .iter()
        .next()
        .expect("hero column");
    assert!(
        super::harness::subtree(world, hero_column)
            .iter()
            .any(|entity| world
                .get::<Text>(*entity)
                .is_some_and(|text| text.as_str() == "—")),
        "the hero column does not guess an owner from an unattributed recording"
    );
    assert!(!std::fs::read_dir(&directory).unwrap().any(|file| {
        file.unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "brp")
    }));
}

#[test]
fn a_missing_profile_mode_never_invents_zero_rated_games() {
    let service = crate::main_menu::tests::service_with_two_rated_modes();
    let mut client = super::menu::signed_up(&service, "missing_mode");
    act(&mut client, Act::Go(Page::Profile));
    client.frames(5);
    crate::main_menu::tests::profile_with_only_one_mode(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
    );
    client.frames(2);
    let words = crate::main_menu::tests::page_words(client.world_mut());
    assert!(words.iter().any(|word| word == "41 rated games"));
    assert!(
        !words.iter().any(|word| word == "0 rated games"),
        "the absent mode has no invented statistics: {words:?}"
    );
}

#[test]
fn a_browser_history_card_survives_blob_failure_with_details_unavailable() {
    use bevy::prelude::*;
    let directory = fresh_library("browser_quota_history_ui");
    crate::main_menu::tests::persist_browser_history_with_a_failed_blob(&directory);
    let mut client = local_menu();
    crate::main_menu::tests::install_local_library(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
        &directory,
    );
    act(&mut client, Act::Go(Page::Profile));
    client.within(
        std::time::Duration::from_secs(15),
        "the cached history row is drawn",
        |client| {
            crate::main_menu::tests::page_words(client.world_mut())
                .iter()
                .any(|word| word == "Shadow Fiend")
        },
    );
    assert_eq!(
        client.world().non_send::<MenuCore>().last_played_hero(),
        Some(bota_proto::HeroId::SHADOW_FIEND)
    );
    let world = client.world_mut();
    let row = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "profile match quota-history")
        .expect("the cached match remains drawn")
        .0;
    assert!(
        world.get::<crate::main_menu::MenuButton>(row).is_none(),
        "a row without retained metadata offers no details link"
    );
    act(&mut client, Act::MatchDetails("quota-history".to_owned()));
    assert_eq!(client.world().non_send::<MenuCore>().page(), Page::Profile);
    assert!(
        !client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible()
    );
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "Match details are unavailable."),
        "an explicit details request reports the missing metadata"
    );
}

#[test]
fn the_avatar_edit_button_opens_its_editor_through_the_pointer() {
    use bevy::prelude::*;
    let service = crate::main_menu::tests::service();
    let mut client = super::menu::signed_up(&service, "avatar_pointer");
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .avatar_backend(true);
    act(&mut client, Act::Go(Page::Profile));
    client.hover_ui_with(std::time::Duration::from_secs(20), |client| {
        let world = client.world_mut();
        world
            .query_filtered::<(Entity, &Name), With<NativeButton>>()
            .iter(world)
            .find(|(_, name)| name.as_str() == "edit avatar")
            .map(|(entity, _)| entity)
    });
    client.click(MouseButton::Left);
    client.within(
        std::time::Duration::from_secs(20),
        "the avatar editor opened",
        |client| {
            client
                .shown_texts()
                .iter()
                .any(|text| text == "CHANGE AVATAR")
        },
    );
}

#[test]
fn recent_match_cards_receive_the_pointer() {
    use bevy::prelude::*;
    let mut client = design_preview("home-local");
    let row = client.hover_ui_with(std::time::Duration::from_secs(20), |client| {
        let world = client.world_mut();
        world
            .query_filtered::<(Entity, &Name), With<NativeButton>>()
            .iter(world)
            .find(|(_, name)| name.as_str().starts_with("recent "))
            .map(|(entity, _)| entity)
    });
    assert!(
        client
            .world()
            .get::<bevy::picking::hover::DirectlyHovered>(row)
            .is_some_and(|hover| hover.get()),
        "the match row is the actual pointer target"
    );
}

#[test]
fn leaderboard_player_click_and_keyboard_open_the_served_read_only_profile() {
    use bevy::input::ButtonState;
    use bevy::prelude::*;
    use bevy::text::FontSize;
    let service = crate::main_menu::tests::service();
    let (account, expected) = crate::main_menu::tests::served_leaderboard_profile(&service);
    let mut client = super::menu::signed_up(&service, "profile_viewer");
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .avatar_backend(true);
    act(&mut client, Act::Go(Page::Leaderboard));
    let wanted = format!("leaderboard profile {account}");
    client.until("the service supplies the ranked player row", |client| {
        let world = client.world_mut();
        world
            .query_filtered::<&Name, With<NativeButton>>()
            .iter(world)
            .any(|name| name.as_str() == wanted)
    });
    let rank = crate::main_menu::tests::rank_in_served_board(
        client.world().non_send::<MenuCore>(),
        account,
    );
    assert_ne!(
        account, rank,
        "the served account id must differ from its actual rank"
    );
    let name = format!("leaderboard profile {account}");
    let world = client.world_mut();
    let row = world
        .query_filtered::<(Entity, &Name), With<NativeButton>>()
        .iter(world)
        .find(|(_, label)| label.as_str() == name)
        .map(|(entity, _)| entity);
    assert!(
        row.is_some(),
        "a public leaderboard account is a clickable row"
    );
    assert!(
        world
            .get::<bevy::input_focus::tab_navigation::TabIndex>(row.expect("row"))
            .is_some(),
        "the row participates in native focus"
    );
    let original_cache =
        crate::main_menu::tests::own_cached_profile_id(client.world().non_send::<MenuCore>());
    client.click_named(&name);
    assert_eq!(
        client.world().non_send::<MenuCore>().page(),
        Page::PlayerProfile(account)
    );
    let profile_ready = |client: &mut super::harness::Client| {
        let world = client.world_mut();
        world
            .query::<(&Text, &TextFont)>()
            .iter(world)
            .any(|(text, font)| {
                text.0 == expected && matches!(font.font_size,FontSize::Px(size)if size>=28.0)
            })
    };
    client.within(
        std::time::Duration::from_secs(20),
        "target profile loaded from the service",
        profile_ready,
    );
    client.until(
        "the served public avatar is drawn without an editor",
        |client| {
            crate::main_menu::tests::drawn_profile_avatar(client.world_mut())
                == Some([60, 130, 210, 255])
        },
    );
    assert_eq!(
        crate::main_menu::tests::own_cached_profile_id(client.world().non_send::<MenuCore>()),
        original_cache,
        "visitor replies never overwrite the owner profile cache"
    );
    let world = client.world_mut();
    let controls: Vec<_> = world
        .query_filtered::<&Name, With<NativeButton>>()
        .iter(world)
        .map(ToString::to_string)
        .collect();
    for forbidden in ["edit avatar", "SIGN OUT", "CHANGE SERVICE"] {
        assert!(
            !controls
                .iter()
                .any(|name| name.eq_ignore_ascii_case(forbidden)),
            "another account is read-only: {forbidden}"
        );
    }
    client.click_named("back");
    assert_eq!(
        client.world().non_send::<MenuCore>().page(),
        Page::Leaderboard
    );
    let focused_name = |client: &super::harness::Client| {
        client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get()
            .and_then(|entity| client.world().get::<Name>(entity))
            .map(|name| name.as_str().to_owned())
    };
    for _ in 0..32 {
        if focused_name(&client).as_deref() == Some(name.as_str()) {
            break;
        }
        client.key(KeyCode::Tab, ButtonState::Pressed);
        client.key(KeyCode::Tab, ButtonState::Released);
        client.frames(2);
    }
    assert_eq!(
        focused_name(&client).as_deref(),
        Some(name.as_str()),
        "Tab reaches the same player row across redraws"
    );
    client.key(KeyCode::Enter, ButtonState::Pressed);
    client.key(KeyCode::Enter, ButtonState::Released);
    client.frames(3);
    client.within(
        std::time::Duration::from_secs(20),
        "keyboard opens the same served profile",
        profile_ready,
    );
    client.click_named("back");
    let gamepad = client
        .world_mut()
        .spawn((
            bevy::input::gamepad::Gamepad::default(),
            bevy::input::gamepad::GamepadSettings::default(),
        ))
        .id();
    let pad = |client: &mut super::harness::Client, button, value| {
        client
            .world_mut()
            .write_message(bevy::input::gamepad::RawGamepadEvent::Button(
                bevy::input::gamepad::RawGamepadButtonChangedEvent::new(gamepad, button, value),
            ));
        client.frames(1);
    };
    for _ in 0..32 {
        if focused_name(&client).as_deref() == Some(name.as_str()) {
            break;
        }
        pad(
            &mut client,
            bevy::input::gamepad::GamepadButton::DPadDown,
            1.0,
        );
        pad(
            &mut client,
            bevy::input::gamepad::GamepadButton::DPadDown,
            0.0,
        );
        client.frames(2);
    }
    assert_eq!(
        focused_name(&client).as_deref(),
        Some(name.as_str()),
        "a gamepad reaches the same player row"
    );
    pad(&mut client, bevy::input::gamepad::GamepadButton::South, 1.0);
    pad(&mut client, bevy::input::gamepad::GamepadButton::South, 0.0);
    client.within(
        std::time::Duration::from_secs(20),
        "gamepad opens the same served profile",
        profile_ready,
    );
    act(&mut client, Act::Go(Page::Profile));
    client.frames(4);
    let world = client.world_mut();
    assert!(
        world
            .query_filtered::<&Name, With<NativeButton>>()
            .iter(world)
            .any(|name| name.as_str() == "sign out"),
        "the own profile still has account controls"
    );
    assert!(
        world
            .query_filtered::<&Name, With<NativeButton>>()
            .iter(world)
            .any(|name| name.as_str() == "edit avatar"),
        "visiting somebody else never disables the owner's editor"
    );
}

#[test]
fn a_history_results_player_opens_the_public_profile_and_back_restores_the_match() {
    let service = crate::main_menu::tests::portable_service();
    let (account, expected) = crate::main_menu::tests::registered_public_profile(&service);
    let mut client = super::menu::signed_up(&service, "history_viewer");
    crate::main_menu::tests::use_declared_public_fixture_identity(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
        &service,
    );
    let mut metadata = crate::postgame::preview::metadata("match_details");
    metadata.extend(
        bota_proto::encode_frame_to_vec(&(
            13_u8,
            service.public_identity.as_deref(),
            vec![(0_u8, Some(account))],
        ))
        .expect("appended provenance"),
    );
    crate::main_menu::tests::install_profile_history(
        &mut client.world_mut().non_send_mut::<MenuCore>(),
        metadata,
    );
    act(&mut client, Act::Go(Page::MatchHistory));
    client.frames(5);
    client.click_named("history DETAILS");
    client.click_named("results scoreboard");
    client.click_named("results profile 0");
    client.within(
        std::time::Duration::from_secs(20),
        "the history player's served profile",
        |client| client.shown_texts().iter().any(|text| text == &expected),
    );
    assert!(
        !client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible()
    );
    client.click_named("back");
    assert!(
        client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible(),
        "Back restores the inspected match"
    );
    assert!(client.shown_texts().iter().any(|text| text == "NET WORTH"));
    client
        .world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    let focused_name = |client: &super::harness::Client| {
        client
            .world()
            .resource::<bevy::input_focus::InputFocus>()
            .get()
            .and_then(|entity| client.world().get::<bevy::prelude::Name>(entity))
            .map(|name| name.as_str().to_owned())
    };
    for _ in 0..64 {
        if focused_name(&client).as_deref() == Some("results profile 0") {
            break;
        }
        client.key(
            bevy::prelude::KeyCode::Tab,
            bevy::input::ButtonState::Pressed,
        );
        client.key(
            bevy::prelude::KeyCode::Tab,
            bevy::input::ButtonState::Released,
        );
        client.frames(2);
    }
    assert_eq!(
        focused_name(&client).as_deref(),
        Some("results profile 0"),
        "keyboard focus stays in the result modal and reaches its profile name"
    );
    client.key(
        bevy::prelude::KeyCode::Enter,
        bevy::input::ButtonState::Pressed,
    );
    client.key(
        bevy::prelude::KeyCode::Enter,
        bevy::input::ButtonState::Released,
    );
    client.within(
        std::time::Duration::from_secs(20),
        "keyboard opens the same public account",
        |client| client.shown_texts().iter().any(|text| text == &expected),
    );
    client.click_named("back");
    client.click_named("results close");
    assert_eq!(
        client.world().non_send::<MenuCore>().page(),
        Page::MatchHistory
    );
}

#[test]
fn foreign_and_corrupt_replay_profiles_never_supply_connection_instructions() {
    use bevy::prelude::*;
    let service = crate::main_menu::tests::service();
    let mut client = super::menu::signed_up(&service, "replay_viewer");
    let foreign = std::net::TcpListener::bind("127.0.0.1:0").expect("foreign destination trap");
    foreign.set_nonblocking(true).expect("nonblocking");
    let prefix = "wss://foreign.example/";
    let uri = format!(
        "{prefix}{}",
        "x".repeat(512 - prefix.len() - "/control".len()) + "/control"
    );
    let original =
        crate::main_menu::tests::own_cached_profile_id(client.world().non_send::<MenuCore>());
    let good =
        bota_proto::encode_frame_to_vec(&(13_u8, Some(uri.as_str()), vec![(0_u8, Some(42_u64))]))
            .expect("foreign provenance");
    for (tail, foreign_valid) in [
        (Vec::new(), false),
        (
            bota_proto::encode_frame_to_vec(&(
                13_u8,
                Some(format!(
                    "ws://{}",
                    foreign.local_addr().expect("trap address")
                )),
                vec![(0_u8, Some(42_u64))],
            ))
            .expect("private claim"),
            false,
        ),
        (
            bota_proto::encode_frame_to_vec(&(13_u8, None::<&str>, vec![(0_u8, None::<u64>)]))
                .expect("local"),
            false,
        ),
        ([good.clone(), vec![4, 0]].concat(), false),
        (
            bota_proto::encode_frame_to_vec(&(
                13_u8,
                Some("wss://ex\u{202e}ample.org"),
                vec![(0_u8, Some(42_u64))],
            ))
            .expect("corrupt"),
            false,
        ),
        (good, true),
    ] {
        let mut metadata = crate::postgame::preview::metadata("match_details");
        metadata.extend(tail);
        crate::main_menu::tests::install_profile_history(
            &mut client.world_mut().non_send_mut::<MenuCore>(),
            metadata,
        );
        act(&mut client, Act::Go(Page::MatchHistory));
        client.frames(5);
        client.click_named("history DETAILS");
        client.click_named("results scoreboard");
        let world = client.world_mut();
        let links = world
            .query_filtered::<&Name, With<NativeButton>>()
            .iter(world)
            .filter(|name| name.as_str().starts_with("results profile "))
            .count();
        assert_eq!(
            crate::main_menu::tests::public_profile_request_count(
                client.world().non_send::<MenuCore>()
            ),
            0,
            "listing and rendering metadata never requests profiles"
        );
        if foreign_valid {
            assert_eq!(links, 1);
            client.click_named("results profile 0");
            assert!(
                client.shown_texts().iter().any(|text| text.contains(&uri)),
                "foreign matchmaker identity is visible as plain text"
            );
            assert!(
                client
                    .shown_texts()
                    .iter()
                    .any(|text| text.contains("The file claims") && text.contains("not verified"))
            );
            let world = client.world_mut();
            let (_, node, layout) = world
                .query::<(&Text, &ComputedNode, &bevy::text::TextLayoutInfo)>()
                .iter(world)
                .find(|(text, _, _)| text.contains(&uri))
                .expect("drawn foreign notice");
            assert!(
                layout.size.x <= node.size().x + 1.0,
                "a maximum-length identity fits the notice width"
            );
            assert!(
                layout.size.y <= node.size().y + 1.0,
                "the complete identity fits the notice height"
            );
            assert!(
                client
                    .world()
                    .non_send::<crate::postgame::PostGame>()
                    .visible()
            );
            let world = client.world_mut();
            assert!(
                world
                    .query_filtered::<&Name, With<NativeButton>>()
                    .iter(world)
                    .any(|name| name.as_str() == "results dismiss"),
                "the readable notice can be dismissed without closing the result"
            );
            let focus = client
                .world()
                .resource::<bevy::input_focus::InputFocus>()
                .get()
                .and_then(|entity| client.world().get::<Name>(entity))
                .map(|name| name.as_str());
            assert_eq!(
                focus,
                Some("results dismiss"),
                "the notice takes keyboard focus from the covered result controls"
            );
            client.key(KeyCode::Enter, bevy::input::ButtonState::Pressed);
            client.key(KeyCode::Enter, bevy::input::ButtonState::Released);
            client.frames(3);
            assert!(
                client
                    .world()
                    .non_send::<crate::postgame::PostGame>()
                    .visible()
            );
            assert!(!client.shown_texts().iter().any(|text| text.contains(&uri)));
        } else {
            assert_eq!(
                links, 0,
                "old, local, or corrupt metadata supplies no profile buttons"
            );
        }
        client.frames(8);
        assert!(
            matches!(foreign.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "replay identity must never be dialled"
        );
        assert_eq!(
            crate::main_menu::tests::public_profile_request_count(
                client.world().non_send::<MenuCore>()
            ),
            0,
            "a foreign click never queries the configured service either"
        );
        assert_eq!(
            crate::main_menu::tests::own_cached_profile_id(client.world().non_send::<MenuCore>()),
            original
        );
        client.click_named("results close");
    }
}

#[test]
fn public_profile_layout_shows_public_ratings_without_account_edits_or_invented_heroes() {
    use bevy::prelude::*;
    for name in ["profile_other", "profile_other_unavailable"] {
        let mut client = super::harness::Client::run(&["--menu-preview", name]);
        client.until("the public profile fixture is drawn", |client| {
            client.shown_texts().iter().any(|text| {
                text == if name == "profile_other" {
                    "mira#2207"
                } else {
                    "Player #42"
                }
            })
        });
        let world = client.world_mut();
        let controls: Vec<_> = world
            .query_filtered::<&Name, With<NativeButton>>()
            .iter(world)
            .map(ToString::to_string)
            .collect();
        for forbidden in ["edit avatar", "sign out", "change service"] {
            assert!(
                !controls
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(forbidden))
            );
        }
        assert!(controls.iter().any(|name| name == "back"));
        if name == "profile_other" {
            assert!(client.shown_texts().iter().any(|text| text == "1,524"));
            assert!(client.shown_texts().iter().any(|text| text == "1,302"));
            let words = crate::main_menu::tests::page_words(client.world_mut());
            assert!(
                !words
                    .iter()
                    .any(|text| matches!(text.as_str(), "Unknown player" | "WATCH" | "DETAILS")),
                "public rows leave missing hero facts unknown and offer no unavailable artifact actions"
            );
            assert!(
                !words
                    .iter()
                    .any(|text| matches!(text.as_str(), "Pudge" | "Shadow Fiend")),
                "public match lists contain no hero identity to invent: {words:?}"
            );
        } else {
            assert!(
                client
                    .shown_texts()
                    .iter()
                    .any(|text| text == "This player's profile is unavailable")
            );
        }
    }
}

#[test]
fn a_served_live_postmatch_profile_and_real_public_match_list_preserve_the_live_session() {
    use bevy::prelude::*;
    let service = crate::main_menu::tests::service_with(|config| {
        config.replace(
            "BOTA_SEATS = \"{seats}\"",
            "BOTA_SEATS = \"{seats}\", BOTA_PROFILE_ACCOUNTS = \"{accounts}\"",
        )
    });
    let played = crate::main_menu::tests::served_postmatch(&service);
    let id = played.id;
    let account = played.other;
    let slot = played.other_slot;
    let expected = played.other_name;
    let mut client = super::harness::Client::run_with_core(&[], played.game);
    client.world_mut().insert_non_send(played.menu);
    {
        let mut post = client
            .world_mut()
            .non_send_mut::<crate::postgame::PostGame>();
        post.expect(crate::postgame::Origin::Service(
            bota_radovid::proto::MatchId(id),
        ));
        post.begin();
    }
    client.until(
        "the actual live postgame has its recorded player names",
        |client| {
            client
                .world()
                .non_send::<crate::postgame::PostGame>()
                .visible()
                && client
                    .world()
                    .non_send::<crate::postgame::PostGame>()
                    .summary()
                    .is_some_and(|summary| summary.rows.len() == 2)
        },
    );
    client.click_named("results scoreboard");
    let tick = client.core().view().expect("final view").tick;
    let owner = client.core().my_slot;
    let orders = client.core().shown_orders.len();
    let typing = *client.world().resource::<crate::input::Typing>();
    assert_eq!(client.world().non_send::<MenuCore>().page(), Page::Game);
    assert!(client.world().non_send::<MenuCore>().in_game());
    assert!(matches!(
        client.core().source,
        bota_client_core::Source::Live(_)
    ));
    client.click_named(&format!("results profile {slot}"));
    client.within(
        std::time::Duration::from_secs(20),
        "the service supplies the opponent profile and its real match",
        |client| {
            client.shown_texts().iter().any(|text| text == &expected)
                && crate::main_menu::tests::visitor_public_matches(
                    client.world().non_send::<MenuCore>(),
                )
                .is_some_and(|matches| matches.len() == 1)
        },
    );
    let rows =
        crate::main_menu::tests::visitor_public_matches(client.world().non_send::<MenuCore>())
            .expect("served matches");
    assert_eq!(rows[0].file, format!("public-{id}"));
    assert_eq!(rows[0].mode, "solo_mid");
    assert!(rows[0].hero.is_none());
    assert!(rows[0].kda.is_none());
    assert!(rows[0].net_worth.is_none());
    assert!(!rows[0].watchable);
    assert!(
        rows[0].details_available,
        "served match facts can be inspected without a game report"
    );
    client.frames(3);
    client.click_named(&format!("profile match public-{id}"));
    assert!(
        client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible(),
        "a profile reached from a live result can inspect its served match too"
    );
    assert_eq!(
        client.world().non_send::<MenuCore>().page(),
        Page::PlayerProfile(account)
    );
    client.click_named("results close");
    assert!(
        !client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible()
    );
    assert_eq!(
        client.world().non_send::<MenuCore>().page(),
        Page::PlayerProfile(account)
    );
    client.key(KeyCode::KeyS, bevy::input::ButtonState::Pressed);
    client.key(KeyCode::KeyS, bevy::input::ButtonState::Released);
    client.click(MouseButton::Right);
    client.frames(3);
    client.click_named("back");
    assert_eq!(client.world().non_send::<MenuCore>().page(), Page::Game);
    assert!(client.world().non_send::<MenuCore>().in_game());
    assert!(
        client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible()
    );
    assert!(matches!(
        client.core().source,
        bota_client_core::Source::Live(_)
    ));
    assert!(
        client.shown_texts().iter().any(|text| text == "NET WORTH"),
        "Back restores the selected scoreboard page"
    );
    assert_eq!(client.core().my_slot, owner);
    assert_eq!(
        client.core().view().expect("same completed match").tick,
        tick
    );
    assert_eq!(client.core().shown_orders.len(), orders);
    assert!(
        *client.world().resource::<crate::input::Typing>() == typing,
        "profile navigation preserves the game input mode"
    );
}

#[test]
fn launcher_public_identity_reaches_the_real_replay_and_its_profile_link() {
    use bota_replay::meta::decode;
    let service = crate::main_menu::tests::portable_service();
    let played = crate::main_menu::tests::served_postmatch(&service);
    let raw = bota_client_core::meta_in_replay(&mut std::io::Cursor::new(&played.replay))
        .expect("read raw server replay")
        .expect("explicitly enabled trailer");
    let provenance = decode(&raw)
        .expect("raw server metadata")
        .profiles()
        .expect("the launcher supplied public provenance");
    assert_eq!(provenance.matchmaker(), Some("wss://match.example/control"));
    assert_eq!(provenance.account(played.other_slot), Some(played.other));
    assert_eq!(
        decode(&played.metadata).expect("server sidecar").profiles(),
        Some(provenance)
    );
    let mut viewer = super::menu::signed_up(&service, "replay_viewer");
    crate::main_menu::tests::use_declared_public_fixture_identity(
        &mut viewer.world_mut().non_send_mut::<MenuCore>(),
        &service,
    );
    crate::main_menu::tests::install_profile_history(
        &mut viewer.world_mut().non_send_mut::<MenuCore>(),
        raw,
    );
    act(&mut viewer, Act::Go(Page::MatchHistory));
    viewer.frames(4);
    viewer.click_named("history DETAILS");
    viewer.click_named("results scoreboard");
    viewer.click_named(&format!("results profile {}", played.other_slot));
    viewer.within(
        std::time::Duration::from_secs(20),
        "the replay profile is served by that same authenticated matchmaker",
        |viewer| {
            viewer
                .shown_texts()
                .iter()
                .any(|text| text == &played.other_name)
        },
    );
}

#[test]
fn profile_timeout_throttle_malformed_and_late_replies_keep_the_visitor_isolated() {
    use bota_radovid::proto::{AccountId, AccountKind, PublicAccount, Reply};
    let service = crate::main_menu::tests::service();
    let (account, expected) = crate::main_menu::tests::registered_public_profile(&service);
    let mut client = super::menu::signed_up(&service, "lifecycle_viewer");
    let original =
        crate::main_menu::tests::own_cached_profile_id(client.world().non_send::<MenuCore>());
    let begin = |client: &mut super::harness::Client| {
        let mut core = client.world_mut().non_send_mut::<MenuCore>();
        core.inspect_player(crate::main_menu::tests::profile_target(&service, account))
            .expect("same service");
        core.hold_profile_account_request()
    };
    let _generation = begin(&mut client);
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .expire_profile_account_request();
    client.frames(3);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "Profile request timed out. Go back and retry.")
    );
    assert_eq!(
        crate::main_menu::tests::public_profile_request_count(
            client.world().non_send::<MenuCore>()
        ),
        1
    );
    client.click_named("back");

    let generation = begin(&mut client);
    {
        let mut core = client.world_mut().non_send_mut::<MenuCore>();
        core.throttle_profile_account_request(generation);
        core.retry_profile_account_request(false);
    }
    client.frames(3);
    assert_eq!(
        crate::main_menu::tests::public_profile_request_count(
            client.world().non_send::<MenuCore>()
        ),
        2,
        "not retried before its deadline"
    );
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .retry_profile_account_request(true);
    client.within(
        std::time::Duration::from_secs(20),
        "a due retry reads the real public profile",
        |client| client.shown_texts().iter().any(|text| text == &expected),
    );
    assert!(
        crate::main_menu::tests::public_profile_request_count(
            client.world().non_send::<MenuCore>()
        ) >= 3
    );
    client.click_named("back");

    let generation = begin(&mut client);
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .inject_profile_account_reply(generation, Reply::Matches { list: Vec::new() });
    client.frames(3);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "The service returned an invalid profile response")
    );
    client.click_named("back");

    let old = begin(&mut client);
    client.frames(2);
    client.click_named("back");
    let current = begin(&mut client);
    let reply = |nickname: &str| Reply::Account {
        account: PublicAccount {
            id: AccountId(account),
            nickname: nickname.into(),
            kind: AccountKind::Human,
            online: false,
            bot_version: None,
            ratings: Vec::new(),
            avatar: None,
        },
    };
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .inject_profile_account_reply(old, reply("stale reply"));
    client.frames(3);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "Loading profile…")
    );
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text == &format!("stale reply#{account}"))
    );
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .inject_profile_account_reply(current, reply("current reply"));
    client.frames(3);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == &format!("current reply#{account}"))
    );
    assert_eq!(
        crate::main_menu::tests::own_cached_profile_id(client.world().non_send::<MenuCore>()),
        original
    );
}

#[test]
fn a_profile_reply_with_a_different_account_id_is_rejected() {
    use bota_radovid::proto::{AccountId, AccountKind, PublicAccount, Reply};
    let service = crate::main_menu::tests::service();
    let (account, _) = crate::main_menu::tests::registered_public_profile(&service);
    let mut client = super::menu::signed_up(&service, "wrong_id_viewer");
    let generation = {
        let mut core = client.world_mut().non_send_mut::<MenuCore>();
        core.inspect_player(crate::main_menu::tests::profile_target(&service, account))
            .expect("same service");
        core.hold_profile_account_request()
    };
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .inject_profile_account_reply(
            generation,
            Reply::Account {
                account: PublicAccount {
                    id: AccountId(account + 1000),
                    nickname: "wrong_player".into(),
                    kind: AccountKind::Human,
                    online: false,
                    bot_version: None,
                    ratings: Vec::new(),
                    avatar: None,
                },
            },
        );
    client.frames(4);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text == "The service returned an invalid profile response")
    );
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text.contains("wrong_player"))
    );
    assert_eq!(
        crate::main_menu::tests::public_profile_request_count(
            client.world().non_send::<MenuCore>()
        ),
        1
    );
}

fn dismiss_notice_without_closing_results(input: u8) {
    use bevy::{input::ButtonState, prelude::*};
    let mut client =
        super::harness::Client::run(&["--menu-preview", "match_details_foreign_profile"]);
    client.until("the notice is drawn", |client| {
        client
            .shown_texts()
            .iter()
            .any(|text| text.contains("The file claims"))
    });
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text.contains("The file claims"))
    );
    match input {
        0 => {
            client.key(KeyCode::Escape, ButtonState::Pressed);
            client.key(KeyCode::Escape, ButtonState::Released);
        }
        1 => {
            client.key(KeyCode::Enter, ButtonState::Pressed);
            client.key(KeyCode::Enter, ButtonState::Released);
        }
        _ => {
            let gamepad = client
                .world_mut()
                .spawn((
                    bevy::input::gamepad::Gamepad::default(),
                    bevy::input::gamepad::GamepadSettings::default(),
                ))
                .id();
            for value in [1.0, 0.0] {
                client
                    .world_mut()
                    .write_message(bevy::input::gamepad::RawGamepadEvent::Button(
                        bevy::input::gamepad::RawGamepadButtonChangedEvent::new(
                            gamepad,
                            bevy::input::gamepad::GamepadButton::South,
                            value,
                        ),
                    ));
                client.frames(2);
            }
        }
    }
    client.frames(4);
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text.contains("The file claims"))
    );
    assert!(
        client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible()
    );
    assert!(client.shown_texts().iter().any(|text| text == "NET WORTH"));
}
#[test]
fn escape_dismisses_the_notice_without_closing_results() {
    dismiss_notice_without_closing_results(0);
}
#[test]
fn enter_dismisses_the_notice_without_closing_results() {
    dismiss_notice_without_closing_results(1);
}
#[test]
fn gamepad_activation_dismisses_the_notice_without_closing_results() {
    dismiss_notice_without_closing_results(2);
}

#[test]
fn an_unavailable_profile_notice_is_spaced_below_its_header() {
    use bevy::prelude::*;
    let mut client = design_preview("profile_other_unavailable");
    let world = client.world_mut();
    let back = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "back")
        .expect("back")
        .0;
    let header = world.get::<ChildOf>(back).expect("header").parent();
    let header_bottom = world
        .get::<bevy::ui::UiGlobalTransform>(header)
        .expect("header position")
        .translation
        .y
        + world
            .get::<ComputedNode>(header)
            .expect("header box")
            .size()
            .y
            / 2.0;
    let notice = world
        .query::<(Entity, &Text)>()
        .iter(world)
        .find(|(_, text)| text.as_str() == "This player's profile is unavailable")
        .expect("notice")
        .0;
    let notice_top = world
        .get::<bevy::ui::UiGlobalTransform>(notice)
        .expect("notice position")
        .translation
        .y
        - world
            .get::<ComputedNode>(notice)
            .expect("notice box")
            .size()
            .y
            / 2.0;
    assert!(
        notice_top - header_bottom >= 12.0,
        "notice gap is {}",
        notice_top - header_bottom
    );
}
#[test]
fn the_foreign_claim_notice_panel_fits_its_text_and_dismiss_button() {
    use bevy::prelude::*;
    let mut client =
        super::harness::Client::run(&["--menu-preview", "match_details_foreign_profile"]);
    client.until("the notice is drawn", |client| {
        client
            .shown_texts()
            .iter()
            .any(|text| text.contains("The file claims"))
    });
    let world = client.world_mut();
    let panel = world
        .query::<(&bevy::input_focus::tab_navigation::TabGroup, &ComputedNode)>()
        .iter(world)
        .find(|(group, _)| group.modal && group.order == 1)
        .expect("notice modal")
        .1
        .size()
        .y;
    let text = world
        .query::<(&Text, &bevy::text::TextLayoutInfo)>()
        .iter(world)
        .find(|(text, _)| text.contains("The file claims"))
        .expect("notice text")
        .1
        .size
        .y;
    let button = world
        .query::<(&Name, &ComputedNode)>()
        .iter(world)
        .find(|(name, _)| name.as_str() == "results dismiss")
        .expect("dismiss")
        .1
        .size()
        .y;
    let overhead = panel - text - button;
    assert!(
        (24.0..=96.0).contains(&overhead),
        "notice chrome should fit its content; extra height is {overhead}"
    );
}

#[test]
fn a_hidden_profile_is_not_bound_in_the_server_replay_or_restamped_by_the_client() {
    use bevy::prelude::*;
    let service = crate::main_menu::tests::portable_service();
    let mut played = crate::main_menu::tests::served_postmatch_visibility(&service, true);
    let raw = bota_replay::meta::decode(&played.metadata)
        .expect("raw server metadata")
        .profiles()
        .expect("known matchmaker");
    assert_eq!(
        raw.account(played.other_slot),
        None,
        "the game launch must not publish a hidden profile binding"
    );
    let replay = bota_client_core::meta_in_replay(&mut std::io::Cursor::new(&played.replay))
        .expect("replay")
        .expect("explicit trailer");
    assert_eq!(
        bota_replay::meta::decode(&replay)
            .expect("trailer")
            .profiles()
            .expect("context")
            .account(played.other_slot),
        None
    );
    let fetched = played
        .menu
        .take_meta()
        .expect("authenticated metadata delivery")
        .bytes
        .expect("metadata");
    assert_eq!(
        bota_replay::meta::decode(&fetched)
            .expect("client metadata")
            .profiles()
            .expect("context")
            .account(played.other_slot),
        None,
        "private participant results must not restore a hidden public binding"
    );
    let mut viewer = super::menu::signed_up(&service, "hidden_replay_viewer");
    crate::main_menu::tests::install_profile_history(
        &mut viewer.world_mut().non_send_mut::<MenuCore>(),
        fetched,
    );
    act(&mut viewer, Act::Go(Page::MatchHistory));
    viewer.frames(4);
    viewer.click_named("history DETAILS");
    viewer.click_named("results scoreboard");
    let world = viewer.world_mut();
    assert!(
        !world
            .query_filtered::<&Name, With<NativeButton>>()
            .iter(world)
            .any(|name| name.as_str() == format!("results profile {}", played.other_slot))
    );
}

#[test]
fn a_profile_keeps_its_match_list_in_the_profile_instead_of_linking_to_replay_import() {
    use bevy::prelude::*;
    let mut client = local_menu();
    act(&mut client, Act::Go(Page::Profile));
    let world = client.world_mut();
    let navigation: Vec<_> = world
        .query_filtered::<&Name, With<NativeButton>>()
        .iter(world)
        .map(ToString::to_string)
        .collect();
    assert!(
        !navigation
            .iter()
            .any(|name| name == "matches" || name == "all matches"),
        "the profile's matches belong to this profile, not another page"
    );
    assert!(client.shown_texts().iter().any(|text| text == "MATCHES"));
    assert_eq!(client.world().non_send::<MenuCore>().page(), Page::Profile);
}

#[test]
fn replay_library_and_match_history_offer_no_file_import_controls() {
    use bevy::prelude::*;
    let mut client = local_menu();
    for page in [Page::Replays, Page::MatchHistory] {
        act(&mut client, Act::Go(page));
        let words = client.shown_texts();
        assert!(
            !words.iter().any(|text| text.contains("Replay file")
                || text.contains("OPEN A REPLAY FILE")
                || text == "OPEN"
                || text == "OPEN A .BRP FILE"
                || text == "DOWNLOAD .BRP"),
            "file import is absent from {page:?}"
        );
        let world = client.world_mut();
        assert!(
            !world
                .query_filtered::<&Name, With<NativeButton>>()
                .iter(world)
                .any(|name| name.as_str() == "open replay file")
        );
    }
}

#[test]
fn a_served_profile_match_row_opens_match_details_without_leaving_the_profile() {
    use bevy::prelude::*;
    let service = crate::main_menu::tests::service();
    let played = crate::main_menu::tests::served_postmatch(&service);
    let id = played.id;
    let account = played.other;
    let mut client = super::menu::signed_up(&service, "profile_match_reader");
    client
        .world_mut()
        .non_send_mut::<MenuCore>()
        .inspect_player(crate::main_menu::tests::profile_target(&service, account))
        .expect("configured matchmaker");
    client.until("served match list", |client| {
        crate::main_menu::tests::visitor_public_matches(client.world().non_send::<MenuCore>())
            .is_some_and(|rows| rows.len() == 1)
    });
    let name = format!("profile match public-{id}");
    let world = client.world_mut();
    assert!(
        world
            .query_filtered::<&Name, With<NativeButton>>()
            .iter(world)
            .any(|label| label.as_str() == name),
        "a real public match is a clickable row"
    );
    client.click_named(&name);
    client.until("match details visible", |client| {
        client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible()
    });
    assert_eq!(
        client.world().non_send::<MenuCore>().page(),
        Page::PlayerProfile(account)
    );
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|text| text.contains(&format!("Match {id}"))),
        "details identify the selected served match"
    );
    client.click_named("results close");
    assert_eq!(
        client.world().non_send::<MenuCore>().page(),
        Page::PlayerProfile(account)
    );
    assert!(
        !client
            .world()
            .non_send::<crate::postgame::PostGame>()
            .visible()
    );
}

#[test]
fn a_players_profile_presents_one_uniform_scrollable_match_table() {
    use bevy::prelude::*;
    let mut client = design_preview("profile");
    let headings = ["HERO PLAYED", "RESULT", "DATE / TIME", "DURATION", "TYPE"];
    let world = client.world_mut();
    let mut positions = Vec::new();
    for label in headings {
        let (_, placed) = world
            .query::<(&Text, &UiGlobalTransform)>()
            .iter(world)
            .find(|(text, _)| text.as_str() == label)
            .unwrap_or_else(|| panic!("missing match table heading {label}"));
        positions.push(placed.translation.x);
    }
    assert!(
        positions.windows(2).all(|pair| pair[0] < pair[1]),
        "the table columns follow the requested reading order"
    );
    let viewport = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "player matches scroll")
        .expect("the match list owns its scroll viewport")
        .0;
    assert!(world.get::<ScrollPosition>(viewport).is_some());
    assert!(
        world
            .query::<&Name>()
            .iter(world)
            .any(|name| name.as_str() == "player matches scroll thumb"),
        "the scrollbar is visible on the right"
    );
    let rows: Vec<_> = world
        .query::<(Entity, &Name, &ComputedNode)>()
        .iter(world)
        .filter(|(_, name, _)| name.as_str().starts_with("profile match "))
        .map(|(entity, _, node)| (entity, node.unrounded_size().y))
        .collect();
    assert!(
        rows.len() > 6,
        "the list is not restricted to the old six-card preview"
    );
    assert!(
        rows.iter()
            .all(|(_, height)| (*height - rows[0].1).abs() < 0.5),
        "every match row has the same height"
    );
    assert!(
        !world
            .query::<&Text>()
            .iter(world)
            .any(|text| text.as_str() == "MOST PLAYED"),
        "the match table uses the profile width"
    );
}

#[test]
fn profile_match_table_headings_align_with_their_data_columns() {
    use bevy::prelude::*;
    let mut client = design_preview("profile");
    let world = client.world_mut();
    let row = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "profile match preview-0.brp")
        .expect("first row")
        .0;
    let columns: Vec<_> = world
        .get::<Children>(row)
        .expect("row columns")
        .iter()
        .collect();
    for (index, label) in ["HERO PLAYED", "RESULT", "DATE / TIME", "DURATION", "TYPE"]
        .into_iter()
        .enumerate()
    {
        let header = world
            .query::<(&Text, &ChildOf)>()
            .iter(world)
            .find(|(text, _)| text.as_str() == label)
            .expect("column header")
            .1
            .parent();
        let left = |entity| {
            let node = world.get::<ComputedNode>(entity).expect("laid out column");
            world
                .get::<UiGlobalTransform>(entity)
                .expect("column position")
                .translation
                .x
                - node.size().x / 2.0
        };
        assert!(
            (left(header) - left(columns[index])).abs() <= 1.0,
            "{label} heading and cells share a column: heading={}, cell={}",
            left(header),
            left(columns[index])
        );
    }
}

#[test]
fn a_profile_match_row_uses_the_shared_hover_feedback() {
    use bevy::prelude::*;
    let mut client = design_preview("profile");
    client.mouse(Vec2::new(4.0, 4.0));
    client.frames(3);
    let world = client.world_mut();
    let row = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "profile match preview-0.brp")
        .expect("first row")
        .0;
    let fill = |client: &super::harness::Client| {
        (
            client.world().get::<BackgroundColor>(row).cloned(),
            client.world().get::<BackgroundGradient>(row).cloned(),
        )
    };
    let before = fill(&client);
    client.hover_ui(row, std::time::Duration::from_secs(10));
    client.frames(3);
    assert_ne!(
        fill(&client),
        before,
        "the same palette feedback as other clickable rows is visible"
    );
}

#[test]
fn the_profile_match_list_scrolls_by_wheel_and_by_dragging_its_thumb() {
    use bevy::{
        input::{
            ButtonState,
            mouse::{MouseScrollUnit, MouseWheel},
        },
        prelude::*,
        window::PrimaryWindow,
    };
    let mut client = design_preview("profile");
    let world = client.world_mut();
    let viewport = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "player matches scroll")
        .expect("viewport")
        .0;
    let thumb = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, name)| name.as_str() == "player matches scroll thumb")
        .expect("thumb")
        .0;
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .expect("window");
    let centre = client.centre_of(viewport).expect("viewport position");
    client.mouse(centre);
    client.frames(3);
    let wheel = MouseWheel {
        phase: bevy::input::touch::TouchPhase::Moved,
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: -4.0,
        window,
    };
    client
        .world_mut()
        .write_message(bevy::window::WindowEvent::MouseWheel(wheel));
    client.world_mut().write_message(wheel);
    client.frames(4);
    let before = client
        .world()
        .get::<ScrollPosition>(viewport)
        .expect("scroll")
        .y;
    assert!(before > 0.0, "wheel input scrolls the actual match list");
    let start = client.centre_of(thumb).expect("thumb position");
    client.mouse(start);
    client.button(MouseButton::Left, ButtonState::Pressed);
    client.mouse(start + Vec2::new(0.0, 90.0));
    client.frames(3);
    client.button(MouseButton::Left, ButtonState::Released);
    client.frames(3);
    assert!(
        client
            .world()
            .get::<ScrollPosition>(viewport)
            .expect("scroll")
            .y
            > before,
        "dragging the real scrollbar moves farther down the table"
    );
}
