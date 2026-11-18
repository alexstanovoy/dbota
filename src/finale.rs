//! The end of a match a fallen Ancient decided, as Dota shows it: the interface steps back and
//! takes no more orders, the camera flies to the falling Ancient and turns slowly about it while
//! its destruction plays, the music pack's stinger plays from the flight, and the caption names
//! the winner (Dota's `dota_hud_game_end` panel: `RADIANT VICTORY`, `DIRE VICTORY`) as the
//! announcer does. Two seconds
//! after the collapse the screen goes dark and the match is left for its result in the menu (a
//! match of the service, one watched live, a replay opened from the menu); a game with no result
//! page there (one of this machine, the command line's) shows its post-game screen instead.
//!
//! A match that ends with no Ancient falling (given away, stopped, at a time limit) has none of it.
//!
//! The sequence runs on the real clock, so it holds its timing whatever the frame rate, and after
//! a long frame it is where the music is. Everything it plays is loaded with the match, never at
//! its start: the stingers and the announcer's lines here, the destruction models with the map's.

use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};
use bevy::prelude::*;
use bevy::ui::{ColorStop, RadialGradient, RadialGradientShape, UiPosition, UiTransform};
use bota_client_core::{FeedKind, Source};
use bota_proto::{Team, UnitKind};

use crate::camera::RtsCamera;
use crate::channels::{Channel, Tuned};
use crate::manifest::{Manifest, cache_path};
use crate::session::Session;

/// How long the camera takes to reach the fallen Ancient, from the decision, in seconds.
pub const FLIGHT_SECS: f32 = 1.2;
/// When the caption comes in, after the decision: Dota's `dota_hud_victory_message_delay`.
pub const CAPTION_SECS: f32 = 2.0;
/// The caption's fade in: `#VictoryLabel`'s opacity transition.
const CAPTION_FADE_SECS: f32 = 0.8;
/// The caption's settling from large to its size: `LabelGrow` (1.8, then 0.92 at a tenth, then 1).
const CAPTION_GROW_SECS: f32 = 5.1;
/// How long the end holds after the Ancient's collapse before the match is left.
pub const HOLD_SECS: f32 = 2.0;
/// The darkening at the very end, within the hold: Dota's `FadeDown`.
const FADE_SECS: f32 = 0.6;
/// The collapse when the destruction model has no clip to time it by: the shorter of the two.
const COLLAPSE_FALLBACK_SECS: f32 = 10.0;
/// The longest the whole sequence may last: the Dire Ancient's collapse (11.6 s) and the hold.
pub const LONGEST_SECS: f32 = 13.7;

/// How far the camera turns about the Ancient over the sequence, in degrees.
const ORBIT_DEGREES: f32 = 40.0;
/// How the camera looks at the Ancient: its distance from the point it looks at, how steeply it
/// looks down (degrees), and how high above the ground that point is (world units). Through the
/// game's long lens (25 degrees) the Ancient stands from the middle of the picture to three
/// quarters down it, under the caption.
const SHOT_DISTANCE: f32 = 3600.0;
const SHOT_PITCH_DEGREES: f32 = 40.0;
const SHOT_HEIGHT: f32 = 200.0;
/// How far above the Ancient the camera aims once over it, so that the Ancient stands in the
/// lower part of the picture.
const AIM_ABOVE: f32 = 250.0;
/// How much the flight rises over a long way, as a share of the way, and at most.
const ARC_SHARE: f32 = 0.3;
const ARC_MAX: f32 = 3000.0;
/// The least height of the camera above the ground beneath it.
const CLEARANCE: f32 = 200.0;

/// The caption's size and its glow, at Dota's 1080 lines: `#VictoryLabel` (90 px, a text shadow
/// of 8 px glowing `#A1FFAC` for the Radiant, black words in `#FF3300` for the Dire).
const CAPTION_PX: f32 = 90.0;
const REFERENCE_HEIGHT: f32 = 1080.0;

/// The interface the game draws: its layers up to the chat's (and the loading screen's), and above
/// the world's own picture (the lowest layer, a scaled render's: `crate::quality`). The menu, the
/// settings and the screens over the game stay above it, and are left alone.
const GAME_LAYERS: std::ops::RangeInclusive<i32> = (i32::MIN + 1)..=50;
/// The caption's own layer: over the game's interface, under the menu's.
const CAPTION_LAYER: i32 = 49;

/// Who sees the end: one of the sides (its own player), or a watcher of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewer {
    Winner,
    Loser,
    Watcher,
}

/// The end being shown.
pub struct Run {
    pub winner: Team,
    pub viewer: Viewer,
    /// The stinger it plays, by its logical name.
    pub cue: String,
    /// Seconds since the decision, on the real clock.
    pub elapsed: f32,
    /// How long it lasts: the collapse and the hold.
    pub length: f32,
    /// The point the camera ends looking at, over the fallen Ancient.
    pub shot: Vec3,
    /// Where the camera was at the decision.
    from: Pose,
    music: Option<Entity>,
    /// The music's own loudness, before the player's balance.
    music_gain: f32,
    announced: bool,
    left: bool,
    ui: Entity,
}

/// What the end shows, if one is shown; and what it plays, loaded ahead.
#[derive(Resource, Default)]
pub struct Finale {
    pub run: Option<Run>,
    sounds: Sounds,
}

impl Finale {
    /// Whether the end is being shown: the post-game screen waits for it.
    pub fn showing(&self) -> bool {
        self.run
            .as_ref()
            .is_some_and(|run| run.elapsed < run.length)
    }
}

/// The stingers and the announcer's lines, by side, loaded with the client.
#[derive(Default)]
struct Sounds {
    win: [Option<Handle<AudioSource>>; 2],
    lose: [Option<Handle<AudioSource>>; 2],
    victory: [Option<Handle<AudioSource>>; 2],
}

/// The loudness of a stinger before the player's balance: its event's own
/// (`valve_dota_001.stinger.*`: 0.6, the Dire's win 0.8).
fn stinger_gain(winner: Team, viewer: Viewer) -> f32 {
    if winner == Team::Dire && viewer != Viewer::Loser {
        0.8
    } else {
        0.6
    }
}

/// Where a camera stands and looks.
#[derive(Clone, Copy, Debug)]
struct Pose {
    focus: Vec3,
    distance: f32,
    pitch: f32,
}

/// The caption's parts.
#[derive(Component)]
struct Caption;
#[derive(Component)]
struct CaptionWords;
#[derive(Component)]
struct Darkening;
/// The root of what the end draws, which the interface's stepping back leaves alone.
#[derive(Component)]
struct FinaleUi;

pub fn plugin(app: &mut App) {
    app.init_resource::<Finale>()
        .add_systems(Startup, load_sounds)
        // Begun in the frame the end arrives, before anything of that frame draws the match.
        .add_systems(PreUpdate, begin.after(crate::session::pump))
        .add_systems(Update, (advance, fly).chain().after(crate::camera::Placed))
        .add_systems(
            PostUpdate,
            step_back.before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
}

fn load_sounds(mut finale: ResMut<Finale>, manifest: Res<Manifest>, server: Res<AssetServer>) {
    let load = |name: String| {
        manifest
            .sound_files(&name)
            .next()
            .map(|file| server.load(cache_path(file)))
    };
    for (at, side) in [Team::Radiant, Team::Dire]
        .map(side_name)
        .into_iter()
        .enumerate()
    {
        finale.sounds.win[at] = load(format!("sound.music.{side}_win"));
        finale.sounds.lose[at] = load(format!("sound.music.{side}_lose"));
        finale.sounds.victory[at] = load(format!("sound.announcer.victory.{side}"));
    }
}

fn side(team: Team) -> usize {
    usize::from(team == Team::Dire)
}

fn side_name(team: Team) -> &'static str {
    ["radiant", "dire"][side(team)]
}

fn other(team: Team) -> Team {
    match team {
        Team::Radiant => Team::Dire,
        _ => Team::Radiant,
    }
}

/// The side a match ended for, when an Ancient fell to end it: the winner of a decided match
/// whose loser's Ancient the feed tells of.
fn decided_by_an_ancient(core: &bota_client_core::App) -> Option<Team> {
    if core.aborted.is_some() {
        return None;
    }
    let winner = match core.over.as_ref()?.0 {
        team @ (Team::Radiant | Team::Dire) => team,
        Team::Neutral => return None,
    };
    let fell = core.feed.iter().any(|line| {
        matches!(
            line.kind,
            FeedKind::StructureFell { team, kind: UnitKind::Ancient, .. } if team == other(winner)
        )
    });
    fell.then_some(winner)
}

/// Who watches: a player in a seat of the match sees it as its side, anyone else as a watcher.
fn viewer_of(core: &bota_client_core::App, winner: Team) -> Viewer {
    let own = matches!(core.source, Source::Live(_))
        .then_some(core.my_slot)
        .flatten()
        .and_then(|slot| core.view()?.player(slot))
        .map(|player| player.team);
    match own {
        Some(team) if team == winner => Viewer::Winner,
        Some(Team::Radiant | Team::Dire) => Viewer::Loser,
        _ => Viewer::Watcher,
    }
}

/// How long the fallen Ancient takes to collapse: its destruction model's clip.
fn collapse_of(bank: &crate::units::ModelBank, fallen: Team) -> f32 {
    let name = match fallen {
        Team::Dire => "ancient.dire.destruction",
        _ => "ancient.radiant.destruction",
    };
    bank.0
        .iter()
        .find(|model| model.model.logical == name)
        .and_then(|model| model.clips.iter().find(|clip| clip.0 == "death"))
        .map_or(COLLAPSE_FALLBACK_SECS, |clip| clip.2)
}

/// Starts the end once a fallen Ancient decided the match, and lets it go with the match.
#[allow(clippy::too_many_arguments)]
fn begin(
    mut commands: Commands,
    session: NonSend<Session>,
    mut finale: ResMut<Finale>,
    bank: Res<crate::units::ModelBank>,
    ground: Res<crate::map::Ground>,
    camera: Single<&RtsCamera>,
    settings: Res<crate::config::Settings>,
    fonts: Res<crate::ui::Fonts>,
) {
    let core = &session.0;
    if core.over.is_none() {
        if let Some(run) = finale.run.take() {
            // The music may have ended by itself, or with the match's sounds.
            for entity in [Some(run.ui), run.music].into_iter().flatten() {
                if let Ok(mut gone) = commands.get_entity(entity) {
                    gone.try_despawn();
                }
            }
        }
        return;
    }
    if finale.run.is_some() {
        return;
    }
    let Some(winner) = decided_by_an_ancient(core) else {
        return;
    };
    let Some(map) = core.session().map(|session| session.map()) else {
        return;
    };
    let fallen = other(winner);
    let Some(ancient) = map.ancients[side(fallen)] else {
        return;
    };
    let (x, y) = (ancient.x.to_f32(), ancient.y.to_f32());
    let shot = Vec3::new(x, ground.at(x, y) + SHOT_HEIGHT, -y);
    let viewer = viewer_of(core, winner);
    // Bounded whatever the install's clip says.
    let length = (collapse_of(&bank, fallen) + HOLD_SECS).min(LONGEST_SECS);
    let (cue, stinger) = match viewer {
        Viewer::Loser => (
            format!("sound.music.{}_lose", side_name(fallen)),
            finale.sounds.lose[side(fallen)].clone(),
        ),
        Viewer::Winner | Viewer::Watcher => (
            format!("sound.music.{}_win", side_name(winner)),
            finale.sounds.win[side(winner)].clone(),
        ),
    };
    let music_gain = stinger_gain(winner, viewer);
    let music = stinger.map(|source| {
        commands
            .spawn((
                Tuned {
                    channel: Channel::Music,
                    base: music_gain,
                },
                AudioPlayer::new(source),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(music_gain)),
                crate::mixer::GameAudio,
            ))
            .id()
    });
    let ui = spawn_caption(&mut commands, &fonts, winner);
    finale.run = Some(Run {
        winner,
        viewer,
        cue,
        elapsed: 0.0,
        length,
        shot,
        from: Pose {
            focus: camera.focus,
            distance: camera.distance,
            pitch: settings.camera.pitch.to_radians(),
        },
        music,
        music_gain,
        announced: false,
        left: false,
        ui,
    });
    if let Some(run) = &finale.run {
        info!(
            "the end: {:?} won, seen by a {:?}, {} plays, {:.1} s",
            run.winner, run.viewer, run.cue, run.length
        );
    }
}

/// The words of the caption: Dota's `dota_game_end_victory_title_*`, in its title case's capitals.
pub fn caption_text(winner: Team) -> &'static str {
    match winner {
        Team::Dire => "DIRE VICTORY",
        _ => "RADIANT VICTORY",
    }
}

/// The words' shadow, which keeps their edge on any ground: dark under white words, the glow's
/// own under black ones.
fn shadow_of(winner: Team) -> Color {
    match winner {
        Team::Dire => Color::srgb_u8(0xFF, 0x33, 0x00),
        _ => Color::BLACK,
    }
}

/// The caption's colours: the words' and their glow's.
fn caption_colours(winner: Team) -> (Color, Color) {
    match winner {
        Team::Dire => (Color::BLACK, Color::srgb_u8(0xFF, 0x33, 0x00)),
        _ => (Color::WHITE, Color::srgb_u8(0xA1, 0xFF, 0xAC)),
    }
}

/// What stands behind the words, `shown` of the way in: Dota's `#VictoryBackgroundScene`, a
/// dark radial wash of the winner's colour; the Dire's black words stand on their orange glow.
fn halo(winner: Team, shown: f32) -> BackgroundGradient {
    let (_, glow) = caption_colours(winner);
    let (centre, middle) = match winner {
        Team::Dire => (glow.with_alpha(0.85), glow.with_alpha(0.35)),
        _ => (Color::srgba(0.02, 0.10, 0.08, 0.72), glow.with_alpha(0.16)),
    };
    let faded = |colour: Color| colour.with_alpha(colour.alpha() * shown);
    BackgroundGradient::from(RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::Ellipse(Val::Percent(50.0), Val::Percent(50.0)),
        vec![
            ColorStop::new(faded(centre), Val::Percent(0.0)),
            ColorStop::new(faded(middle), Val::Percent(55.0)),
            ColorStop::new(Color::NONE, Val::Percent(100.0)),
        ],
    ))
}

fn spawn_caption(commands: &mut Commands, fonts: &crate::ui::Fonts, winner: Team) -> Entity {
    let (words, _) = caption_colours(winner);
    commands
        .spawn((
            FinaleUi,
            Name::new("finale"),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            GlobalZIndex(CAPTION_LAYER),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                Caption,
                Name::new("finale caption"),
                Node {
                    // Dota's label sits a little above the middle of the screen.
                    margin: UiRect::bottom(Val::Percent(14.0)),
                    padding: UiRect::axes(Val::Percent(14.0), Val::Percent(5.0)),
                    ..default()
                },
                halo(winner, 0.0),
                UiTransform::IDENTITY,
                Visibility::Hidden,
                Pickable::IGNORE,
            ))
            .with_child((
                CaptionWords,
                Text::new(caption_text(winner)),
                TextFont {
                    font: fonts.face(crate::theme::Face::Display).clone().into(),
                    font_size: FontSize::Px(CAPTION_PX),
                    ..default()
                },
                TextColor(words.with_alpha(0.0)),
                TextShadow {
                    offset: Vec2::splat(2.0),
                    color: shadow_of(winner).with_alpha(0.0),
                },
                Pickable::IGNORE,
            ));
            root.spawn((
                Darkening,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::BLACK.with_alpha(0.0)),
                Pickable::IGNORE,
            ));
        })
        .id()
}

/// Smooth from 0 to 1 and still at both ends, with no overshoot (Perlin's smootherstep).
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    (t * t * t * (t * (t * 6.0 - 15.0) + 10.0)).clamp(0.0, 1.0)
}

/// The caption's scale at `t` seconds after it came in: `LabelGrow`'s keyframes, eased.
fn caption_scale(t: f32) -> f32 {
    let knee = CAPTION_GROW_SECS * 0.1;
    if t < knee {
        1.8 + (0.92 - 1.8) * ease(t / knee)
    } else {
        0.92 + (1.0 - 0.92) * ease((t - knee) / (CAPTION_GROW_SECS - knee))
    }
}

/// Moves the end on by the real time that passed, and leaves the match at its end.
#[allow(clippy::too_many_arguments)]
fn advance(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut finale: ResMut<Finale>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    scale: Res<UiScale>,
    mut caption: Query<(&mut Visibility, &mut UiTransform, &mut BackgroundGradient), With<Caption>>,
    mut words: Query<(&mut TextColor, &mut TextShadow, &mut TextFont), With<CaptionWords>>,
    mut dark: Query<&mut BackgroundColor, With<Darkening>>,
    mut tuned: Query<&mut Tuned>,
    menu: Option<NonSendMut<crate::main_menu::MenuCore>>,
) {
    let finale = &mut *finale;
    let Some(run) = finale.run.as_mut() else {
        return;
    };
    run.elapsed += time.delta_secs();
    let t = run.elapsed;
    let shown = t >= CAPTION_SECS;
    let since = t - CAPTION_SECS;
    let opacity = if shown {
        ease(since / CAPTION_FADE_SECS)
    } else {
        0.0
    };
    let (colour, _) = caption_colours(run.winner);
    let shadow_colour = shadow_of(run.winner);
    for (mut visibility, mut transform, mut backdrop) in &mut caption {
        visibility.set_if_neq(crate::ui::visible(shown));
        let scale = caption_scale(since.max(0.0));
        transform.scale = Vec2::splat(scale);
        if shown && since <= CAPTION_FADE_SECS + 0.1 {
            *backdrop = halo(run.winner, opacity);
        }
    }
    // Dota's 90 pixels at 1080 lines, whatever the window and the interface's scale.
    let size = CAPTION_PX * window.height() / REFERENCE_HEIGHT / scale.0.max(0.1);
    for (mut text, mut shadow, mut font) in &mut words {
        text.0 = colour.with_alpha(opacity);
        shadow.color = shadow_colour.with_alpha(opacity * 0.8);
        if font.font_size != FontSize::Px(size) {
            font.font_size = FontSize::Px(size);
        }
    }
    let darkness = ease((t - (run.length - FADE_SECS)) / FADE_SECS);
    for mut veil in &mut dark {
        veil.0 = Color::BLACK.with_alpha(darkness);
    }
    if let Some(music) = run.music
        && let Ok(mut tuned) = tuned.get_mut(music)
    {
        tuned.base = run.music_gain * (1.0 - darkness);
    }
    if shown && !run.announced {
        run.announced = true;
        if let Some(line) = finale.sounds.victory[side(run.winner)].clone() {
            commands.spawn((
                Tuned {
                    channel: Channel::Announcer,
                    base: 1.0,
                },
                AudioPlayer::new(line),
                PlaybackSettings::DESPAWN,
                crate::mixer::GameAudio,
            ));
        }
    }
    if t >= run.length && !run.left {
        run.left = true;
        if let Some(mut menu) = menu {
            menu.end_shown();
        }
    }
}

/// Where the camera is at `t` seconds into the end, and the point it aims at: flown from where
/// it was to the shot of the Ancient, eased in and out, rising over a long way; turning about the
/// Ancient all along, slowly at first and at the last; never below the ground's clearance.
fn pose_at(run: &Run, t: f32, ground: &crate::map::Ground) -> (Vec3, Vec3) {
    let flown = ease(t / FLIGHT_SECS);
    let focus = run.from.focus.lerp(run.shot, flown);
    let travel = run.from.focus.distance(run.shot);
    let arc = (travel * ARC_SHARE).min(ARC_MAX) * (std::f32::consts::PI * flown).sin();
    let distance = run.from.distance + (SHOT_DISTANCE - run.from.distance) * flown + arc;
    let pitch = run.from.pitch + (SHOT_PITCH_DEGREES.to_radians() - run.from.pitch) * flown;
    let yaw = ORBIT_DEGREES.to_radians() * ease(t / run.length);
    let rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-pitch);
    let mut at = focus + rotation * Vec3::Z * distance;
    at.y = at.y.max(ground.at(at.x, -at.z) + CLEARANCE);
    (at, focus + Vec3::Y * (AIM_ABOVE * flown))
}

/// Places the camera for the end, over what the player's camera would do; the player's camera
/// is left over the Ancient, as the end leaves it.
fn fly(
    finale: Res<Finale>,
    ground: Res<crate::map::Ground>,
    mut camera: Single<(&mut RtsCamera, &mut Transform)>,
) {
    let Some(run) = finale.run.as_ref() else {
        return;
    };
    let t = run.elapsed.min(run.length);
    let (at, aim) = pose_at(run, t, &ground);
    let (rts, transform) = &mut *camera;
    rts.focus = run.from.focus.lerp(run.shot, ease(t / FLIGHT_SECS));
    **transform = Transform::from_translation(at).looking_at(aim, Vec3::Y);
}

/// The game's interface steps back while the end is shown: every part of it is hidden, the
/// end's own drawing and the screens above the game (the menu, the settings) aside. What it hid
/// is shown again as it was once the end is over.
#[allow(clippy::type_complexity)]
fn step_back(
    finale: Res<Finale>,
    mut roots: Query<
        (Entity, &mut Visibility, Option<&GlobalZIndex>),
        (With<Node>, Without<ChildOf>, Without<FinaleUi>),
    >,
    mut hidden: Local<Vec<(Entity, Visibility)>>,
) {
    if !finale.showing() {
        for (entity, was) in hidden.drain(..) {
            if let Ok((_, mut visibility, _)) = roots.get_mut(entity) {
                visibility.set_if_neq(was);
            }
        }
        return;
    }
    for (entity, mut visibility, layer) in &mut roots {
        let layer = layer.map_or(0, |layer| layer.0);
        if GAME_LAYERS.contains(&layer) && *visibility != Visibility::Hidden {
            if !hidden.iter().any(|(held, _)| *held == entity) {
                hidden.push((entity, *visibility));
            }
            *visibility = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ease_is_still_at_both_ends_and_never_overshoots() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        let mut last = 0.0;
        for step in 1..=1000 {
            let value = ease(step as f32 / 1000.0);
            assert!(value + 1e-6 >= last, "monotone: {value} after {last}");
            assert!(value <= 1.0, "never past 1: {value}");
            last = value;
        }
        assert!(ease(0.001) < 1e-6, "no jump at the start");
        assert!(1.0 - ease(0.999) < 1e-6, "no jump at the end");
    }
}
