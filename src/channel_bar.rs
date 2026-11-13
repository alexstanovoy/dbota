//! Dota's channel bar (`dota_hud_channelbar`): while one's hero channels an ability that runs a
//! known time (a Watcher lit or put out, an Outpost captured, a Twin Gate), the ability's round icon
//! and a bar that fills with the channel, above the hero's panel at the bottom of the screen, with
//! the time left written in it (`DOTA_Hud_ChannelBar`: "Channeling {s:seconds_left}", centred in the
//! well, white with a shadow). The time is the simulation's: the ticks the channel has run (the
//! unit's [`bota_proto::ChannelView`]) and the client's interpolated clock between two snapshots, so
//! a channel cancelled is gone at once, and a replay at any speed counts as its snapshots come. A
//! teleport has its own bar over the hero (see `bars`).

use bevy::prelude::*;
use bota_client_core::App as Core;
use bota_proto::AbilityId;

use crate::dota_text::Texts;
use crate::manifest::Manifest;
use crate::session::{MatchKnown, Session};
use crate::ui::{self, Fonts, dota};

/// The sizes of the bar in Dota's pixels: the icon's disc and where it stands, and the well of
/// the bar, its border and where it starts (Dota's `#ChannelbarContainer` is 380 wide with a
/// 30 high well; its icon is shrunk to fit the well, and the bar stands just above the hero's
/// panel).
const WIDTH: f32 = 380.0;
const HEIGHT: f32 = 64.0;
const ICON: f32 = 60.0;
const ICON_LEFT: f32 = 6.0;
const WELL: (f32, f32) = (300.0, 30.0);
const WELL_LEFT: f32 = 70.0;
/// Where the bar's bottom edge stands above the window's.
const BOTTOM: f32 = 250.0;
const WELL_BORDER: f32 = 2.0;
const WELL_COLOUR: Color = Color::srgba(0.0, 0.0, 0.0, 0.933);
/// The label's size, Dota's default label size (`#ChannelBarLabel` sets none).
const LABEL_SIZE: f32 = 18.0;

pub fn plugin(app: &mut App) {
    app.add_systems(First, reset.in_set(crate::loading::Begin))
        .add_systems(Startup, spawn)
        .add_systems(PostUpdate, show.run_if(|known: Res<MatchKnown>| known.0));
}

fn reset(mut bars: Query<&mut Visibility, With<ChannelBar>>) {
    for mut bar in &mut bars {
        *bar = Visibility::Hidden;
    }
}

/// The whole bar.
#[derive(Component)]
pub struct ChannelBar;

/// The bar's round icon.
#[derive(Component)]
struct ChannelIcon;

/// The part of the bar's well that is full, in percent of its width.
#[derive(Component)]
pub struct ChannelFill;

/// The words in the bar's well: "Channeling" and the seconds left.
#[derive(Component)]
pub struct ChannelLabel;

fn spawn(mut commands: Commands, fonts: Res<Fonts>) {
    let bar = commands
        .spawn((
            ChannelBar,
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                bottom: dota(BOTTOM),
                width: dota(WIDTH),
                height: dota(HEIGHT),
                margin: UiRect::left(dota(-WIDTH / 2.0)),
                ..default()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .id();
    let well = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: dota(WELL_LEFT),
                top: dota((HEIGHT - WELL.1) / 2.0),
                width: dota(WELL.0),
                height: dota(WELL.1),
                border: UiRect::all(dota(WELL_BORDER)),
                border_radius: BorderRadius::all(dota(3.0)),
                ..default()
            },
            BackgroundColor(WELL_COLOUR),
            BorderColor::all(WELL_COLOUR),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.6),
                px(0),
                px(0),
                px(0),
                dota(6.0),
            ),
            Pickable::IGNORE,
            ChildOf(bar),
        ))
        .id();
    let stop = |r: u8, g: u8, b: u8, at: f32| ColorStop::new(Color::srgb_u8(r, g, b), percent(at));
    commands.spawn((
        ChannelFill,
        Node {
            width: percent(0),
            height: percent(100),
            border_radius: BorderRadius::all(dota(4.0)),
            ..default()
        },
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            stop(0x5D, 0x78, 0x7E, 0.0),
            stop(0x9D, 0xC0, 0xCB, 20.0),
            stop(0x96, 0xBA, 0xC3, 50.0),
            stop(0x68, 0x8B, 0x8F, 100.0),
        ])),
        Pickable::IGNORE,
        ChildOf(well),
    ));
    commands.spawn((
        ChannelLabel,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        ui::text("", &fonts.bold, LABEL_SIZE * ui::DOTA, Color::WHITE),
        ui::shadow(),
        TextLayout::justify(Justify::Center),
        Pickable::IGNORE,
        ChildOf(well),
    ));
    commands.spawn((
        ChannelIcon,
        Node {
            position_type: PositionType::Absolute,
            left: dota(ICON_LEFT),
            top: dota((HEIGHT - ICON) / 2.0),
            width: dota(ICON),
            height: dota(ICON),
            border: UiRect::all(dota(3.0)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        ImageNode::default(),
        BorderColor::all(WELL_COLOUR),
        Pickable::IGNORE,
        ChildOf(bar),
    ));
}

/// The channel of one's hero that the bar shows: its ability and how far it has run, in ticks and
/// in all (the simulation's count of ticks run, and the clock's share of the tick under way).
fn channel(core: &Core) -> Option<(AbilityId, f32, u32)> {
    let hero = core.my_hero()?;
    let (unit, _) = core.known(hero)?;
    let channel = unit.channel?;
    let ticks = bota_core::ability_def(channel.ability)?.duration;
    (ticks > 0).then(|| {
        let ran = channel.elapsed as f32 + core.smooth.alpha();
        (channel.ability, ran.min(ticks as f32), ticks)
    })
}

/// The label's words: Dota's `DOTA_Hud_ChannelBar` where the cache has it, its English otherwise,
/// the seconds left to a tenth.
fn words(texts: Option<&Texts>, seconds_left: f32) -> String {
    let left = format!("{:.1}", seconds_left.max(0.0));
    texts
        .and_then(|texts| texts.label("DOTA_Hud_ChannelBar", &[("seconds_left", left.clone())]))
        .unwrap_or_else(|| format!("Channeling {left}"))
}

fn show(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    texts: Option<Res<Texts>>,
    mut icon_of: Local<Option<AbilityId>>,
    mut bar: Single<&mut Visibility, With<ChannelBar>>,
    mut fill: Single<&mut Node, With<ChannelFill>>,
    mut icon: Single<&mut ImageNode, With<ChannelIcon>>,
    mut label: Single<&mut Text, With<ChannelLabel>>,
) {
    let core = &session.0;
    let shown = channel(core);
    bar.set_if_neq(crate::ui::visible(shown.is_some()));
    let Some((ability, ran, ticks)) = shown else {
        return;
    };
    fill.width = percent(ran / ticks as f32 * 100.0);
    let seconds_left = (ticks as f32 - ran) / f32::from(core.tick_rate().max(1));
    label.set_if_neq(Text(words(texts.as_deref(), seconds_left)));
    if *icon_of != Some(ability) {
        *icon_of = Some(ability);
        if let Some(path) = manifest.ability_icon(ability.0) {
            icon.image = server.load(path);
        }
    }
}
