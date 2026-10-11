//! The settings overlay (F10) and the lobby card: sound, interface size, the
//! aids, the keys, and where the art and the sound come from.

use bevy::prelude::*;
use bota_client_core::Phase;

use crate::config::{BINDINGS, Settings, key_name};
use crate::hud::Blocker;
use crate::input::Panels;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// One line of the overlay that can be changed.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    VolumeDown,
    VolumeUp,
    Mute,
    ScaleDown,
    ScaleUp,
    Numbers,
    Voice,
    EdgePan,
}

/// The overlay.
#[derive(Component)]
struct Overlay;

/// The text of the overlay's values.
#[derive(Component)]
struct Values;

/// The lobby card.
#[derive(Component)]
struct LobbyCard;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, (show, clicks, values, lobby));
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    path: Res<crate::SettingsPath>,
) {
    let overlay = commands
        .spawn((
            Overlay,
            Blocker,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                left: percent(30),
                right: percent(30),
                top: percent(12),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(18)),
                row_gap: px(8),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.06, 0.97)),
            BorderColor::all(ui::EDGE),
            GlobalZIndex(20),
            Visibility::Hidden,
        ))
        .id();
    commands.spawn((
        ui::text("SETTINGS", &fonts.bold, 20.0, ui::INK),
        ChildOf(overlay),
    ));
    commands.spawn((
        Values,
        ui::text("", &fonts.regular, 14.0, ui::INK),
        ChildOf(overlay),
    ));
    let buttons = commands
        .spawn((
            Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(6),
                row_gap: px(6),
                ..default()
            },
            ChildOf(overlay),
        ))
        .id();
    for (row, label) in [
        (Row::VolumeDown, "Volume -"),
        (Row::VolumeUp, "Volume +"),
        (Row::Mute, "Mute"),
        (Row::ScaleDown, "Interface -"),
        (Row::ScaleUp, "Interface +"),
        (Row::Numbers, "Damage numbers"),
        (Row::Voice, "Voice responses"),
        (Row::EdgePan, "Edge pan"),
    ] {
        let b = commands
            .spawn((
                row,
                Button,
                Node {
                    padding: UiRect::axes(px(10), px(5)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(ui::PANEL_DEEP),
                BorderColor::all(ui::EDGE),
                ChildOf(buttons),
            ))
            .id();
        commands.spawn((ui::text(label, &fonts.bold, 13.0, ui::INK), ChildOf(b)));
    }
    let keys: Vec<String> = BINDINGS
        .iter()
        .map(|(_, name, key)| format!("{} {}", name.replace('_', " "), key_name(*key)))
        .collect();
    commands.spawn((
        ui::text(
            format!("KEYS (defaults)\n{}", keys.join("   ")),
            &fonts.regular,
            12.0,
            ui::INK_DIM,
        ),
        ChildOf(overlay),
    ));
    let file = path.0.as_ref().map_or("no settings file".to_string(), |p| {
        format!("Settings file: {} (keys are set there)", p.display())
    });
    commands.spawn((
        ui::text(
            format!("{}\n{file}", manifest.census()),
            &fonts.regular,
            12.0,
            ui::INK_DIM,
        ),
        ChildOf(overlay),
    ));
    commands.spawn((
        LobbyCard,
        ui::text("", &fonts.bold, 20.0, ui::INK),
        ui::shadow(),
        Node {
            position_type: PositionType::Absolute,
            left: percent(10),
            top: percent(20),
            ..default()
        },
    ));
}

fn show(panels: Res<Panels>, mut overlay: Single<&mut Visibility, With<Overlay>>) {
    let want = if panels.settings {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if **overlay != want {
        **overlay = want;
    }
}

/// Clicks on the overlay's buttons change the settings; closing it saves.
fn clicks(
    rows: Query<(&Interaction, &Row), Changed<Interaction>>,
    mut settings: ResMut<Settings>,
    panels: Res<Panels>,
    path: Res<crate::SettingsPath>,
    mut was_open: Local<bool>,
    mut dirty: Local<bool>,
) {
    for (interaction, row) in &rows {
        if *interaction != Interaction::Pressed || !panels.settings {
            continue;
        }
        *dirty = true;
        let s = &mut *settings;
        match row {
            Row::VolumeDown => s.volume = (s.volume - 0.1).max(0.0),
            Row::VolumeUp => s.volume = (s.volume + 0.1).min(1.0),
            Row::Mute => s.muted = !s.muted,
            Row::ScaleDown => s.ui_scale = (s.ui_scale - 0.1).max(0.5),
            Row::ScaleUp => s.ui_scale = (s.ui_scale + 0.1).min(2.0),
            Row::Numbers => s.aids.numbers = !s.aids.numbers,
            Row::Voice => s.voice = !s.voice,
            Row::EdgePan => s.aids.edge_pan = !s.aids.edge_pan,
        }
    }
    if *was_open && !panels.settings && *dirty {
        *dirty = false;
        if let Some(path) = &path.0
            && let Err(err) = crate::config::save(path, &settings)
        {
            warn!("settings not saved to {}: {err}", path.display());
        }
    }
    *was_open = panels.settings;
}

/// The overlay's current values.
fn values(settings: Res<Settings>, mut text: Single<&mut Text, With<Values>>) {
    if !settings.is_changed() {
        return;
    }
    let on = |b: bool| if b { "on" } else { "off" };
    let a = &settings.aids;
    text.0 = format!(
        "Volume {:.0}%  {}   Interface {:.0}%\nDamage numbers {}   Voice responses {}   Edge pan {}",
        settings.volume * 100.0,
        if settings.muted { "(muted)" } else { "" },
        settings.ui_scale * 100.0,
        on(a.numbers),
        on(settings.voice),
        on(a.edge_pan),
    );
}

/// The lobby: who sits where, and how to pick and ready.
fn lobby(session: NonSend<Session>, mut card: Single<&mut Text, With<LobbyCard>>) {
    let core = &session.0;
    let s = if core.phase == Phase::Lobby {
        let mut lines = vec![
            match (core.my_slot.is_some(), core.ready, core.me.is_some()) {
                (true, true, _) => "Ready - waiting for the others (R to cancel)".to_string(),
                (true, false, _) => {
                    "Pick a hero: [1] Pudge  [2] Shadow Fiend, then [R] when ready".to_string()
                }
                (false, _, true) => "Spectating - waiting for the match to start".to_string(),
                (false, _, false) => "Connecting...".to_string(),
            },
        ];
        for seat in &core.lobby {
            let hero = seat
                .hero
                .and_then(|h| bota_client_core::hero(h.0))
                .map_or("-", |h| h.name);
            let ready = if seat.ready { "  READY" } else { "" };
            let name = if seat.name.is_empty() {
                "open"
            } else {
                seat.name.as_str()
            };
            lines.push(format!(
                "{:?} seat {}: {name} as {hero}{ready}",
                seat.team, seat.slot.0
            ));
        }
        lines.join("\n")
    } else {
        String::new()
    };
    if card.0 != s {
        card.0 = s;
    }
}
