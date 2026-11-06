//! The public Roshan clock beside the minimap, driven by snapshot evidence.
use crate::{
    hud::{Blocker, Docks},
    manifest::Manifest,
    session::Session,
    ui::{self, Fonts},
};
use bevy::prelude::*;
use bota_client_core::LairState;
use bota_proto::ObjectiveKind;

#[derive(Component)]
struct Panel;
#[derive(Component)]
pub(crate) struct Readout;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(Update, update);
}

fn spawn(
    mut commands: Commands,
    docks: Res<Docks>,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
) {
    let panel = commands
        .spawn((
            Panel,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(crate::minimap::SIDE + 5.),
                left: px(0.),
                width: px(crate::minimap::SIDE),
                min_height: px(40.),
                padding: UiRect::all(px(6.)),
                column_gap: px(5.),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(ui::PANEL),
            Visibility::Hidden,
            Interaction::default(),
            Blocker,
            ChildOf(docks.minimap),
        ))
        .id();
    let image = manifest
        .file("hud.roshan")
        .map(|p| ImageNode::new(server.load(p)))
        .unwrap_or_else(|| ImageNode {
            color: Color::NONE,
            ..default()
        });
    commands.spawn((
        Node {
            width: px(24.),
            height: px(24.),
            flex_shrink: 0.,
            ..default()
        },
        image,
        Pickable::IGNORE,
        ChildOf(panel),
    ));
    commands.spawn((
        Readout,
        Node {
            flex_grow: 1.,
            flex_basis: px(0.),
            ..default()
        },
        ui::text("", &fonts.regular, 12., ui::INK),
        Pickable::IGNORE,
        ChildOf(panel),
    ));
}

fn update(
    session: NonSend<Session>,
    texts: Res<crate::dota_text::Texts>,
    mut panel: Single<&mut Visibility, With<Panel>>,
    mut readout: Single<(&mut Text, &mut TextColor), With<Readout>>,
) {
    let core = &session.0;
    let found = core.view().zip(core.session()).and_then(|(view, session)| {
        view.lairs.iter().find_map(|known| {
            let def = session.map().lairs.get(usize::from(known.lair))?;
            (def.objective == ObjectiveKind::Roshan)
                .then(|| core.lair_status(known.lair).map(|status| (def, status)))
                .flatten()
        })
    });
    let Some((def, status)) = found else {
        **panel = Visibility::Hidden;
        return;
    };
    **panel = Visibility::Inherited;
    let top = if def.day.y > def.night.y {
        def.day
    } else {
        def.night
    };
    let location = texts
        .label(
            if status.home == top {
                "DOTA_Roshan_Top"
            } else {
                "DOTA_Roshan_Bottom"
            },
            &[],
        )
        .unwrap_or_else(|| {
            if status.home == top {
                "Top".into()
            } else {
                "Bottom".into()
            }
        });
    let (key, remaining, color, fallback) = match status.state {
        LairState::Alive => (
            "DOTA_Tooltip_RoshanTimerMessage_RoshanIsAlive",
            None,
            Color::srgb(0.5, 0.9, 0.55),
            "alive",
        ),
        LairState::Dead { remaining } => (
            "DOTA_Tooltip_RoshanTimerMessage_BaseTime",
            Some(remaining),
            Color::srgb(0.95, 0.55, 0.45),
            "dead",
        ),
        LairState::Possible { remaining } => (
            "DOTA_Tooltip_RoshanTimerMessage_VariableTime",
            Some(remaining),
            Color::srgb(0.95, 0.8, 0.4),
            "possibly alive",
        ),
    };
    let timer = remaining
        .map(|ticks| {
            let seconds = ticks.div_ceil(bota_core::rules::TICKS_PER_SECOND);
            format!("{}:{:02}", seconds / 60, seconds % 60)
        })
        .unwrap_or_default();
    let message = texts
        .label(
            key,
            &[
                ("roshan_cave_location", location.clone()),
                ("roshan_respawn_time", timer.clone()),
            ],
        )
        .unwrap_or_else(|| format!("Roshan: {fallback} ({location}) {timer}"));
    let (text, tint) = &mut *readout;
    text.0 = message;
    tint.0 = color;
}
