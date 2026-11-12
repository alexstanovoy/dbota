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
    assert!(
        crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|text| text == "INNATE"),
        "the hero page includes its innate"
    );
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
        crate::main_menu::tests::page_words(client.world_mut())
            .iter()
            .any(|text| text.contains("VICTORY")),
        "details do not require replay bytes"
    );
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
        .find(|(_, name)| name.as_str() == "demo")
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
fn most_played_statistics_fit_inside_their_profile_rows() {
    use bevy::prelude::*;
    let mut client = local_menu();
    client
        .world_mut()
        .insert_resource(crate::main_menu::Preview(
            crate::main_menu::preview("profile").unwrap(),
        ));
    client.frames(8);
    let world = client.world_mut();
    let nodes: Vec<_> = world
        .query::<(Entity, &Name, &ChildOf)>()
        .iter(world)
        .filter(|(_, name, _)| name.as_str() == "most played stats")
        .map(|(entity, _, parent)| (entity, parent.parent()))
        .collect();
    assert_eq!(nodes.len(), 2);
    for (stats, row) in nodes {
        let stats_center = client.centre_of(stats).unwrap();
        let row_center = client.centre_of(row).unwrap();
        let width = |entity| {
            let node = client.world().get::<ComputedNode>(entity).unwrap();
            node.size().x
        };
        assert!(
            stats_center.x + width(stats) / 2.0 <= row_center.x + width(row) / 2.0 + 1.0,
            "the stats must not spill outside the row: stats center={stats_center:?} width={}, row center={row_center:?} width={}",
            width(stats),
            width(row)
        );
    }
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
    assert_eq!(first, "1–13 of 100");
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
    use bota_proto::HeroId;
    let expected = [
        (
            HeroId::PUDGE,
            [
                "25",
                "13",
                "16",
                "+3.0",
                "+1.4",
                "+1.8",
                "70–76",
                "2.2",
                "175",
                "280",
                "1.5 s",
                "670 / 267",
            ],
        ),
        (
            HeroId::SHADOW_FIEND,
            [
                "19",
                "25",
                "16",
                "+2.7",
                "+3.6",
                "+2.2",
                "41–47",
                "4.2",
                "525",
                "305",
                "1.3 s",
                "538 / 267",
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
        let words = crate::main_menu::tests::page_words(client.world_mut());
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
    let words = crate::main_menu::tests::page_words(client.world_mut());
    assert!(
        words.iter().any(|word| word == "Unknown player"),
        "the retained match has a drawn history row: {words:?}"
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
    assert!(
        world
            .query_filtered::<&Name, With<crate::main_menu::Disabled>>()
            .iter(world)
            .any(|name| name.as_str() == "details"),
        "the unavailable details action is visibly disabled"
    );
    act(&mut client, Act::MatchDetails("quota-history".to_owned()));
    assert_eq!(client.world().non_send::<MenuCore>().page(), Page::Profile);
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
