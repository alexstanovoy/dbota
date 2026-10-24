//! The hero panel's portrait while the hero it is about lies dead: the
//! picture without its colour, as the top bar draws a dead hero, with the
//! respawn seconds over it in Dota's red (`#RespawnLabel`). The world keeps
//! its colours.

use bevy::prelude::*;

use crate::hud::{Portrait, lies_dead, subject};
use crate::session::{MatchKnown, Session};
use crate::ui::{self, Fonts};
use crate::wash::Washed;

/// The hero panel's pixels to one of Dota's (`hud.rs`: its portrait is
/// Dota's 159 wide).
const PANEL_PER_DOTA: f32 = crate::hud::PORTRAIT.0 / 159.0;
/// The seconds' size, Dota's `#RespawnLabel`.
const SECONDS_SIZE: f32 = 64.0;
/// The share of the portrait's width left of the numbers down its right
/// side, where the seconds stand.
const SECONDS_WIDTH: f32 = 60.0;
/// The seconds' red (the middle of Dota's `#ff5522ee` to `#cc0000dd`) and
/// their glow.
const SECONDS_INK: Color = Color::srgba(0.9, 0.17, 0.07, 0.95);
const SECONDS_GLOW: Color = Color::srgba(0.208, 0.0, 0.0, 0.87);

/// The washed copy of the portrait, over it while its hero lies dead.
#[derive(Component)]
struct DeadPortrait;

/// The respawn seconds over it.
#[derive(Component)]
struct RespawnSeconds;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, portrait.run_if(|k: Res<MatchKnown>| k.0));
}

/// The washed copy of the portrait and the seconds over it, under what
/// else the portrait holds (its name, its numbers).
pub fn spawn(
    commands: &mut Commands,
    fonts: &Fonts,
    washes: &mut Assets<Washed>,
    portrait: Entity,
) {
    let layer = commands
        .spawn((
            DeadPortrait,
            Node {
                display: Display::None,
                align_items: AlignItems::Center,
                ..ui::fill()
            },
            MaterialNode(washes.add(Washed::dead(Handle::default()))),
            Pickable::IGNORE,
            ChildOf(portrait),
        ))
        .id();
    commands.spawn((
        RespawnSeconds,
        Node {
            width: percent(SECONDS_WIDTH),
            justify_content: JustifyContent::Center,
            ..default()
        },
        ui::text(
            "",
            &fonts.regular,
            SECONDS_SIZE * PANEL_PER_DOTA,
            SECONDS_INK,
        ),
        TextLayout::justify(Justify::Center),
        TextShadow {
            offset: Vec2::ZERO,
            color: SECONDS_GLOW,
        },
        Pickable::IGNORE,
        ChildOf(layer),
    ));
}

/// While the panel's hero lies dead: its portrait's picture, cut as the
/// portrait cuts it, washed of its colour, with the respawn seconds.
fn portrait(
    session: NonSend<Session>,
    images: Res<Assets<Image>>,
    mut washes: ResMut<Assets<Washed>>,
    picture: Single<&ImageNode, With<Portrait>>,
    mut layer: Single<(&mut Node, &MaterialNode<Washed>), With<DeadPortrait>>,
    mut seconds: Single<&mut Text, With<RespawnSeconds>>,
) {
    let core = &session.0;
    let rate = u32::from(core.tick_rate().max(1));
    let left = subject(core)
        .0
        .filter(|p| lies_dead(p))
        .map(|p| p.respawn_left.div_ceil(rate));
    let display = if left.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    if layer.0.display != display {
        layer.0.display = display;
    }
    seconds.set_if_neq(Text(left.map_or(String::new(), |s| s.to_string())));
    let cut = match (picture.rect, images.get(&picture.image)) {
        (Some(rect), Some(image)) => {
            let size = image.size_f32();
            Vec4::new(
                rect.min.x / size.x,
                rect.min.y / size.y,
                rect.width() / size.x,
                rect.height() / size.y,
            )
        }
        _ => Washed::WHOLE,
    };
    let stale = washes
        .get(&layer.1.0)
        .is_some_and(|w| w.picture != picture.image || w.cut != cut);
    if stale && let Some(mut wash) = washes.get_mut(&layer.1.0) {
        wash.picture = picture.image.clone();
        wash.cut = cut;
    }
}
