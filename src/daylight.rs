//! Day and night, read off the view's tick by bota's rule: the light's colour
//! turns from the sun's to the moon's, easing when the time of day turns.
//! Dota's own colours are in the cache (`map/mid/lighting.txt`, made from the
//! map's light entities); without them stand-in numbers do. Sight itself needs
//! nothing here: every unit's vision radius already shrinks in the view, and
//! the fog is drawn from it.

use bevy::prelude::*;

use crate::manifest::Manifest;
use crate::session::Session;

/// Seconds the stand-in light takes to turn from day to night or back.
const TURN_SECONDS: f32 = 3.0;
/// How far the stand-in night's light turns towards moonlight blue, 0 to 1.
const NIGHT_TINT: f32 = 0.6;
/// The sun's and the ambient light's share of their daytime brightness by night.
const NIGHT_SUN: f32 = 0.3;
const NIGHT_AMBIENT: f32 = 0.55;

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

/// The light as the map lays it out by day, taken once it stands.
#[derive(Clone, Copy)]
struct DayLook {
    sun: f32,
    sun_color: LinearRgba,
    ambient: f32,
    ambient_color: LinearRgba,
}

pub fn plugin(app: &mut App) {
    let dota = app
        .world()
        .resource::<Manifest>()
        .root
        .as_deref()
        .and_then(|root| crate::cachefs::read_small(root, "map/mid/lighting.txt"))
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| Dota::parse(&text));
    if let Some(dota) = dota {
        app.insert_resource(dota);
    }
    app.init_resource::<Daylight>()
        .add_systems(Update, (turn, light).chain());
}

/// Eases the light towards the time of day of the latest view; the first
/// view of a session is lit as it is, without a turn.
fn turn(
    session: NonSend<Session>,
    dota: Option<Res<Dota>>,
    time: Res<Time>,
    mut daylight: ResMut<Daylight>,
    mut seen: Local<bool>,
) {
    let Some(view) = session.0.view() else {
        *seen = false;
        return;
    };
    let goal = if bota_core::time_of_day(view.tick).night {
        1.0
    } else {
        0.0
    };
    if !*seen {
        *seen = true;
        daylight.night = goal;
        return;
    }
    if daylight.night == goal {
        return;
    }
    let turn = dota.map_or(TURN_SECONDS, |d| d.turn);
    let step = time.delta_secs() / turn;
    daylight.night += (goal - daylight.night).clamp(-step, step);
}

/// Lays the light over the day look by how far it has turned.
fn light(
    daylight: Res<Daylight>,
    dota: Option<Res<Dota>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut suns: Query<&mut DirectionalLight>,
    mut day: Local<Option<DayLook>>,
) {
    let Ok(mut sun) = suns.single_mut() else {
        return;
    };
    let day = *day.get_or_insert_with(|| DayLook {
        sun: sun.illuminance,
        sun_color: sun.color.to_linear(),
        ambient: ambient.brightness,
        ambient_color: ambient.color.to_linear(),
    });
    if !daylight.is_changed() {
        return;
    }
    let night = daylight.night;
    if let Some(dota) = dota {
        sun.color = dota.sun.day.mix(&dota.sun.night, night).into();
        ambient.color = dota.ambient.day.mix(&dota.ambient.night, night).into();
        return;
    }
    let moonlight = Color::srgb(0.55, 0.66, 1.0).to_linear();
    let blue = Color::srgb(0.42, 0.52, 1.0).to_linear();
    let tint = night * NIGHT_TINT;
    sun.illuminance = day.sun * (1.0 - night * (1.0 - NIGHT_SUN));
    sun.color = day.sun_color.mix(&moonlight, tint).into();
    ambient.brightness = day.ambient * (1.0 - night * (1.0 - NIGHT_AMBIENT));
    ambient.color = day.ambient_color.mix(&blue, tint).into();
}
