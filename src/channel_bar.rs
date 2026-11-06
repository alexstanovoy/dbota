//! Dota's channel bar (`dota_hud_channelbar`): while one's hero channels an ability that runs a
//! known time (a Watcher lit or put out, an Outpost captured), the ability's round icon and a
//! bar that fills with the channel, in the middle of the screen. A teleport has its own bar over
//! the hero (see `bars`).

use bevy::prelude::*;
use bota_client_core::App as Core;
use bota_proto::AbilityId;

use crate::anim::Channels;
use crate::manifest::Manifest;
use crate::session::{MatchKnown, Session};
use crate::ui::dota;

/// The sizes of Dota's css, in its pixels: the icon's disc and where it stands, and the well of
/// the bar, its border and where it starts (`#ChannelbarContainer`, `#ChannelBarChannel`).
const WIDTH: f32 = 474.0;
const HEIGHT: f32 = 80.0;
const ICON: f32 = 76.0;
const ICON_LEFT: f32 = 23.0;
const WELL: (f32, f32) = (380.0, 34.0);
const WELL_LEFT: f32 = 94.0;
const WELL_BORDER: f32 = 2.0;
const WELL_COLOUR: Color = Color::srgba(0.0, 0.0, 0.0, 0.933);

/// The channel in the bar: the ability and the tick it began on.
#[derive(Default)]
struct Running(Option<(AbilityId, u32)>);

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(
        PostUpdate,
        show.after(crate::anim::NoteChannels)
            .run_if(|known: Res<MatchKnown>| known.0),
    );
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

fn spawn(mut commands: Commands) {
    let bar = commands
        .spawn((
            ChannelBar,
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                top: percent(50),
                width: dota(WIDTH),
                height: dota(HEIGHT),
                margin: UiRect::new(dota(-WIDTH / 2.0), px(0), dota(-HEIGHT / 2.0), px(0)),
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

/// The channel of one's hero that the bar shows, with how far it has run (0 to 1).
fn channel(core: &Core, channels: &Channels, running: &mut Running) -> Option<(AbilityId, f32)> {
    let view = core.view()?;
    let hero = core.my_hero()?;
    let ability = channels.of(hero);
    // The bar is of the ability channelled, from the tick it was first seen under way.
    running.0 = match (ability, running.0) {
        (Some(id), Some((was, began))) if was == id => Some((id, began)),
        (Some(id), _) => Some((id, view.tick)),
        (None, _) => None,
    };
    let (id, began) = running.0?;
    let ticks = bota_core::ability_def(id)?.duration;
    (ticks > 0).then(|| {
        let ran = view.tick.saturating_sub(began) as f32 + core.smooth.alpha();
        (id, (ran / ticks as f32).clamp(0.0, 1.0))
    })
}

fn show(
    session: NonSend<Session>,
    channels: Res<Channels>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut running: Local<Running>,
    mut icon_of: Local<Option<AbilityId>>,
    mut bar: Single<&mut Visibility, With<ChannelBar>>,
    mut fill: Single<&mut Node, With<ChannelFill>>,
    mut icon: Single<&mut ImageNode, With<ChannelIcon>>,
) {
    let shown = channel(&session.0, &channels, &mut running);
    bar.set_if_neq(crate::ui::visible(shown.is_some()));
    let Some((ability, share)) = shown else {
        return;
    };
    fill.width = percent(share * 100.0);
    if *icon_of != Some(ability) {
        *icon_of = Some(ability);
        if let Some(path) = manifest.ability_icon(ability.0) {
            icon.image = server.load(path);
        }
    }
}
