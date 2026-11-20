//! The end of a match a fallen Ancient decided, as Dota shows it: the interface steps back and
//! takes no more orders, the camera flies to the falling Ancient and turns slowly about it while
//! its destruction plays, the music pack's stinger plays from the flight, and the caption names
//! the winner (Dota's `dota_hud_game_end` panel: `RADIANT VICTORY`, `DIRE VICTORY`) as the
//! announcer does. Ten seconds after the decision (Dota's delay before its end-game panel) the
//! screen goes dark and the match is left for its result in the menu (a match of the service, one
//! watched live, a replay opened from the menu); a game with no result page there (one of this
//! machine, the command line's) shows its post-game screen instead.
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
/// The darkening at the very end, within the hold: Dota's `FadeDown`.
pub const FADE_SECS: f32 = 0.6;
/// How long the end lasts, from the decision to the match being left: Dota's
/// `dota_sf_game_end_delay`, the ten seconds before its end-game panel comes. The collapse goes on
/// under the closing fade where it is longer (the Dire Ancient's 11.6 s).
pub const LONGEST_SECS: f32 = 10.0;

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

/// The screen shakes of each side's destruction (`C_OP_RenderScreenShake` of its `destroyfx`):
/// amplitude in world units, frequency in hertz, how long it lasts in seconds. The Radiant's
/// shakes twice (`radiant_ancient001_destruction`: 5 at 60 for 4; its `_f`: 5 at 35 for 8), the
/// Dire's once while its blast wave is let go (`dire_ancient_base001_destruction_k`: 5 at 10, for
/// its two seconds of emission). Read from the install's particles by hand: the client's particle
/// reader does not take the shake renderer yet.
const SHAKES: [&[(f32, f32, f32)]; 2] =
    [&[(5.0, 60.0, 4.0), (5.0, 35.0, 8.0)], &[(5.0, 10.0, 2.0)]];

/// The caption at Dota's 1080 lines (`dota_hud_game_end.vcss`): `#VictoryLabel` is 90 px,
/// centred in a 1024 by 640 `#WinLabelContainer` raised 120 px above the middle of the screen.
const CAPTION_PX: f32 = 90.0;
const CONTAINER: Vec2 = Vec2::new(1024.0, 640.0);
const RAISED_PX: f32 = 120.0;
const REFERENCE_HEIGHT: f32 = 1080.0;
/// The label's glow, `text-shadow: 0px 0px 8px 5.0 <colour>`: an eight pixel blur of the letters,
/// made five times as strong. It is drawn as copies of the words in the glow's colour laid in rings
/// about them (radius at 1080 lines, copies, opacity of each), which add up to a halo that follows
/// every letter and fades over its eight pixels.
const GLOW_RINGS: [(f32, usize, f32); 3] = [(2.0, 8, 0.45), (4.5, 12, 0.22), (7.5, 16, 0.1)];

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
    pub(crate) began_at: std::time::Duration,
    /// How long it lasts: the collapse and the hold.
    pub length: f32,
    /// When it is left: at its length, or sooner when the player goes on.
    pub ends_at: f32,
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
            .is_some_and(|run| run.elapsed < run.ends_at)
    }

    /// Goes on to what follows the end: it fades out from where it is, then is left.
    pub fn skip(&mut self) {
        if let Some(run) = &mut self.run {
            run.ends_at = run.ends_at.min(run.elapsed + FADE_SECS);
        }
    }
}

/// The stingers and the announcer's lines, by side, loaded with the client.
#[derive(Default)]
struct Sounds {
    win: [Option<Handle<AudioSource>>; 2],
    lose: [Option<Handle<AudioSource>>; 2],
    victory: [Option<Handle<AudioSource>>; 2],
    destruction: [Option<String>; 2],
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

/// The caption's parts: its box (`#WinLabelContainer`), the ground behind the words, the label
/// that settles in size, the words and the copies of them its glow is made of (each at its offset
/// at 1080 lines, and its own opacity).
#[derive(Component)]
struct CaptionBox;
#[derive(Component)]
struct Backdrop;
#[derive(Component)]
struct CaptionLabel;
#[derive(Component)]
struct Words;
#[derive(Component)]
struct Glow {
    offset: Vec2,
    alpha: f32,
}
#[derive(Component)]
struct Darkening;
/// The root of what the end draws, which the interface's stepping back leaves alone.
#[derive(Component)]
struct FinaleUi;
#[derive(Component)]
struct FinaleAudio;

pub fn plugin(app: &mut App) {
    app.init_resource::<Finale>()
        .add_systems(Startup, load_sounds)
        // Begun in the frame the end arrives, before anything of that frame draws the match.
        .add_systems(PreUpdate, begin.after(crate::session::pump))
        .add_systems(
            Update,
            (advance, fly, draw_caption)
                .chain()
                .after(crate::camera::Placed),
        )
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
        finale.sounds.destruction[at] = manifest
            .sounds_with_events(&format!("sound.ancient.destroyed.{side}"))
            .into_iter()
            .map(|(_, event)| event)
            .next();
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

/// Starts the end once a fallen Ancient decided the match, and lets it go with the match.
#[allow(clippy::too_many_arguments)]
fn begin(
    mut commands: Commands,
    session: NonSend<Session>,
    mut finale: ResMut<Finale>,
    ground: Res<crate::map::Ground>,
    camera: Single<&RtsCamera>,
    settings: Res<crate::config::Settings>,
    fonts: Res<crate::ui::Fonts>,
    time: Res<Time<Real>>,
    mut mixer: ResMut<crate::mixer::Mixer>,
    audio: Query<Entity, With<FinaleAudio>>,
) {
    let core = &session.0;
    if core.over.is_none() {
        if let Some(run) = finale.run.take() {
            commands.entity(run.ui).try_despawn();
            stop_audio(&mut commands, &audio);
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
    let length = LONGEST_SECS;
    mixer.only_event(
        &mut commands,
        finale.sounds.destruction[side(fallen)].clone(),
    );
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
                FinaleAudio,
            ))
            .id()
    });
    let ui = spawn_caption(&mut commands, &fonts, winner);
    finale.run = Some(Run {
        winner,
        viewer,
        cue,
        elapsed: 0.0,
        began_at: time.elapsed(),
        length,
        ends_at: length,
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

fn stop_audio(commands: &mut Commands, audio: &Query<Entity, With<FinaleAudio>>) {
    for entity in audio {
        commands.entity(entity).try_despawn();
    }
}

/// The words of the caption: Dota's `dota_game_end_victory_title_*`, in its title case's capitals.
pub fn caption_text(winner: Team) -> &'static str {
    match winner {
        Team::Dire => "DIRE VICTORY",
        _ => "RADIANT VICTORY",
    }
}

/// The caption's colours: the words' and their glow's (`.DireWin #VictoryLabel`: black words
/// in an orange glow).
fn caption_colours(winner: Team) -> (Color, Color) {
    match winner {
        Team::Dire => (Color::BLACK, Color::srgb_u8(0xFF, 0x33, 0x00)),
        _ => (Color::WHITE, Color::srgb_u8(0xA1, 0xFF, 0xAC)),
    }
}

/// How strong the glow is `t` seconds after the caption came in: `LabelGrow` takes the Radiant's
/// glow from 0x99 to full at a tenth of its 5.1 s and down to 0x11 by its end; `LabelGrowDire`
/// keeps the Dire's at full once it is there.
fn glow_strength(winner: Team, t: f32) -> f32 {
    let knee = CAPTION_GROW_SECS * 0.1;
    let (first, last) = (0x99 as f32 / 255.0, 0x11 as f32 / 255.0);
    if t < knee {
        first + (1.0 - first) * ease(t / knee)
    } else if winner == Team::Dire {
        1.0
    } else {
        1.0 + (last - 1.0) * ease((t - knee) / (CAPTION_GROW_SECS - knee))
    }
}

/// What stands behind the words, `shown` of the way in: `#VictoryBackgroundScene`'s ground, a
/// radial gradient from black at its middle to nothing at four fifths of its way out. (Its
/// particle scene, washed in the winner's colour, is not drawn.)
fn backdrop(shown: f32) -> BackgroundGradient {
    BackgroundGradient::from(RadialGradient::new(
        UiPosition::CENTER,
        RadialGradientShape::Ellipse(Val::Percent(50.0), Val::Percent(50.0)),
        vec![
            ColorStop::new(Color::BLACK.with_alpha(shown), Val::Percent(0.0)),
            ColorStop::new(Color::NONE, Val::Percent(80.0)),
        ],
    ))
}

/// The offsets of the glow's copies at 1080 lines, and each copy's opacity.
fn glow_copies() -> impl Iterator<Item = (Vec2, f32)> {
    GLOW_RINGS.into_iter().flat_map(|(radius, copies, alpha)| {
        (0..copies).map(move |at| {
            let turn = std::f32::consts::TAU * (at as f32 + 0.5) / copies as f32;
            (Vec2::from_angle(turn) * radius, alpha)
        })
    })
}

fn spawn_caption(commands: &mut Commands, fonts: &crate::ui::Fonts, winner: Team) -> Entity {
    let (words, glow) = caption_colours(winner);
    let font = TextFont {
        font: fonts.face(crate::theme::Face::Display).clone().into(),
        font_size: FontSize::Px(CAPTION_PX),
        ..default()
    };
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
                CaptionBox,
                Name::new("finale caption"),
                Node {
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                Visibility::Hidden,
                Pickable::IGNORE,
            ))
            .with_children(|caption| {
                caption.spawn((
                    Backdrop,
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    backdrop(0.0),
                    UiTransform::IDENTITY,
                    Pickable::IGNORE,
                ));
                caption
                    .spawn((
                        CaptionLabel,
                        Node::default(),
                        UiTransform::IDENTITY,
                        Pickable::IGNORE,
                    ))
                    .with_children(|label| {
                        for (offset, alpha) in glow_copies() {
                            label.spawn((
                                Glow { offset, alpha },
                                Node {
                                    position_type: PositionType::Absolute,
                                    ..default()
                                },
                                Text::new(caption_text(winner)),
                                font.clone(),
                                TextColor(glow.with_alpha(0.0)),
                                Pickable::IGNORE,
                            ));
                        }
                        // Last, so that it is drawn over its glow.
                        label.spawn((
                            Words,
                            Text::new(caption_text(winner)),
                            font.clone(),
                            TextColor(words.with_alpha(0.0)),
                            Pickable::IGNORE,
                        ));
                    });
            });
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
    mut dark: Query<&mut BackgroundColor, With<Darkening>>,
    mut tuned: Query<&mut Tuned>,
    menu: Option<NonSendMut<crate::main_menu::MenuCore>>,
    mut mixer: ResMut<crate::mixer::Mixer>,
    audio: Query<Entity, With<FinaleAudio>>,
) {
    let finale = &mut *finale;
    let Some(run) = finale.run.as_mut() else {
        return;
    };
    run.elapsed = time.elapsed().saturating_sub(run.began_at).as_secs_f32();
    let t = run.elapsed;
    let shown = t >= CAPTION_SECS;
    let darkness = ease((t - (run.ends_at - FADE_SECS)) / FADE_SECS);
    for mut veil in &mut dark {
        veil.0 = Color::BLACK.with_alpha(darkness);
    }
    if let Some(music) = run.music
        && let Ok(mut tuned) = tuned.get_mut(music)
    {
        tuned.base = run.music_gain * (1.0 - darkness);
    }
    if shown && t < run.ends_at && !run.announced {
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
                FinaleAudio,
            ));
        }
    }
    if t >= run.ends_at && !run.left {
        run.left = true;
        stop_audio(&mut commands, &audio);
        mixer.close(&mut commands);
        run.music = None;
        if let Some(mut menu) = menu {
            menu.end_shown();
        }
    }
}

/// Draws the caption as far as the end has come: the box at Dota's size for the window, the
/// ground behind it fading and growing in, the words fading in and settling from large, their glow
/// as strong as `LabelGrow` has it.
#[allow(clippy::type_complexity)]
fn draw_caption(
    finale: Res<Finale>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    scale: Res<UiScale>,
    mut boxes: Query<(&mut Visibility, &mut Node), With<CaptionBox>>,
    mut backdrops: Query<
        (&mut UiTransform, &mut BackgroundGradient),
        (With<Backdrop>, Without<CaptionLabel>),
    >,
    mut labels: Query<&mut UiTransform, With<CaptionLabel>>,
    mut words: Query<(&mut TextColor, &mut TextFont), (With<Words>, Without<Glow>)>,
    mut glows: Query<(&Glow, &mut TextColor, &mut TextFont, &mut Node), Without<CaptionBox>>,
) {
    let Some(run) = finale.run.as_ref() else {
        return;
    };
    let since = run.elapsed - CAPTION_SECS;
    let shown = since >= 0.0;
    let opacity = if shown {
        ease(since / CAPTION_FADE_SECS)
    } else {
        0.0
    };
    // Dota's sizes at 1080 lines, whatever the window and the interface's scale.
    let k = window.height() / REFERENCE_HEIGHT / scale.0.max(0.1);
    let size = FontSize::Px(CAPTION_PX * k);
    for (mut visibility, mut node) in &mut boxes {
        visibility.set_if_neq(crate::ui::visible(shown));
        let (width, height) = (Val::Px(CONTAINER.x * k), Val::Px(CONTAINER.y * k));
        if node.width != width || node.height != height {
            node.width = width;
            node.height = height;
            node.margin = UiRect::bottom(Val::Px(2.0 * RAISED_PX * k));
        }
    }
    if !shown {
        return;
    }
    // The ground comes in as the words do, grown from a fifth of its size.
    for (mut transform, mut gradient) in &mut backdrops {
        transform.scale = Vec2::splat(0.2 + 0.8 * opacity);
        if since <= CAPTION_FADE_SECS + 0.1 {
            *gradient = backdrop(opacity);
        }
    }
    for mut transform in &mut labels {
        transform.scale = Vec2::splat(caption_scale(since));
    }
    let (colour, glow) = caption_colours(run.winner);
    for (mut text, mut font) in &mut words {
        text.0 = colour.with_alpha(opacity);
        if font.font_size != size {
            font.font_size = size;
        }
    }
    let strength = opacity * glow_strength(run.winner, since);
    for (copy, mut text, mut font, mut node) in &mut glows {
        text.0 = glow.with_alpha(copy.alpha * strength);
        if font.font_size != size {
            font.font_size = size;
            node.left = Val::Px(copy.offset.x * k);
            node.top = Val::Px(copy.offset.y * k);
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
    let shake = shake_at(other(run.winner), run.elapsed);
    **transform = Transform::from_translation(at + shake).looking_at(aim + shake, Vec3::Y);
}

/// How far the fall shakes the camera `t` seconds after it: each of the side's shakes, fading to
/// nothing over its length.
fn shake_at(fallen: Team, t: f32) -> Vec3 {
    SHAKES[side(fallen)]
        .iter()
        .filter(|(_, _, length)| t < *length)
        .map(|&(amplitude, frequency, length)| {
            let phase = std::f32::consts::TAU * frequency * t;
            let strength = amplitude * (1.0 - t / length);
            Vec3::new(
                phase.sin(),
                (phase * 1.3 + 1.0).sin(),
                (phase * 0.7 + 2.0).sin(),
            ) * strength
        })
        .sum()
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
