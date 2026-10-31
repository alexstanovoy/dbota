//! Day and night, read off the view's tick by bota's rule: the light's colour
//! turns from the sun's to the moon's, easing when the time of day turns.
//! Dota's own colours are in the cache (`map/mid/lighting.txt`, made from the
//! map's light entities); without them the map stays in daylight. Sight itself needs
//! nothing here: every unit's vision radius already shrinks in the view, and
//! the fog is drawn from it.

use bevy::prelude::*;

use crate::manifest::Manifest;
use crate::session::Session;

/// How far the light has turned towards night: 0 day, 1 night.
#[derive(Resource, Default)]
pub struct Daylight {
    pub night: f32,
}

/// Dota's colours for one light, by day and by night, in linear light.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pair {
    day: LinearRgba,
    night: LinearRgba,
}

/// The sun and the ambient light of Dota's map, from `lighting.txt`.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
struct Dota {
    sun: Pair,
    ambient: Pair,
    /// Seconds the light takes to turn.
    turn: f32,
}

impl Dota {
    /// The cache's lighting file; `None` when it is missing or malformed.
    fn parse(text: &str) -> Option<Self> {
        let lighting = crate::cache_text::lighting(text)?;
        // Dota's colours scale the daytime brightness this client sets for its own
        // lights: the day's brightest channel is 1, the night keeps its ratio to it.
        let linear = |[r, g, b, scale]: [f32; 4]| {
            Color::srgb(r / 255.0, g / 255.0, b / 255.0).to_linear() * scale
        };
        let pair = |[day, night]: [[f32; 4]; 2]| {
            let (day, night) = (linear(day), linear(night));
            let top = day.red.max(day.green).max(day.blue);
            let pair = Pair {
                day: (day * (1.0 / top)).with_alpha(1.0),
                night: (night * (1.0 / top)).with_alpha(1.0),
            };
            let finite = |c: LinearRgba| c.to_f32_array().iter().all(|v| v.is_finite());
            (top > 0.0 && finite(pair.day) && finite(pair.night)).then_some(pair)
        };
        Some(Self {
            sun: pair(lighting.sun)?,
            ambient: pair(lighting.ambient)?,
            turn: lighting.turn,
        })
    }
}

/// Whether it is night in the match's view now: Dota's day and night on the game clock, from
/// the horn its map's pregame puts off; none before the match is seen.
pub fn night(core: &bota_client_core::App) -> Option<bool> {
    let view = core.view()?;
    let schedule = bota_core::rules::Schedule::of(core.pregame_ticks());
    Some(bota_core::time_of_day(view.day_tick(), schedule).night)
}

pub fn plugin(app: &mut App) {
    let dota = crate::cachefs::read_small(
        &app.world().resource::<Manifest>().root,
        "map/mid/lighting.txt",
    )
    .and_then(|bytes| String::from_utf8(bytes).ok())
    .and_then(|text| Dota::parse(&text));
    app.init_resource::<Daylight>();
    match dota {
        Some(dota) => {
            app.insert_resource(dota)
                .add_systems(Update, (turn, light).chain());
        }
        None => error!("map/mid/lighting.txt is missing or malformed: the map stays in daylight"),
    }
}

/// Eases the light towards the time of day of the latest view; the first
/// view of a session is lit as it is, without a turn.
fn turn(
    session: NonSend<Session>,
    dota: Res<Dota>,
    time: Res<Time>,
    mut daylight: ResMut<Daylight>,
    mut seen: Local<bool>,
) {
    let Some(night) = night(&session.0) else {
        *seen = false;
        return;
    };
    let goal = if night { 1.0 } else { 0.0 };
    if !*seen {
        *seen = true;
        daylight.night = goal;
        return;
    }
    if daylight.night == goal {
        return;
    }
    let step = time.delta_secs() / dota.turn;
    daylight.night += (goal - daylight.night).clamp(-step, step);
}

/// Lays Dota's light over the map by how far it has turned.
fn light(
    daylight: Res<Daylight>,
    dota: Res<Dota>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut suns: Query<&mut DirectionalLight, With<crate::map::MainSun>>,
) {
    let Ok(mut sun) = suns.single_mut() else {
        return;
    };
    if !daylight.is_changed() {
        return;
    }
    let night = daylight.night;
    sun.color = dota.sun.day.mix(&dota.sun.night, night).into();
    ambient.color = dota.ambient.day.mix(&dota.ambient.night, night).into();
}
