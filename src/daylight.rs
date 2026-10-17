//! Day and night, read off the view's tick by bota's rule: by night the sun
//! dims to a cool moonlight and the ambient light turns blue, as Dota's map
//! does, easing over a few seconds when the time of day turns. Sight itself
//! needs nothing here: every unit's vision radius already shrinks in the
//! view, and the fog is drawn from it.

use bevy::prelude::*;

use crate::session::Session;

/// Seconds the light takes to turn from day to night or back.
const TURN_SECONDS: f32 = 3.0;

/// How far the light has turned towards night: 0 day, 1 night.
#[derive(Resource, Default)]
pub struct Daylight {
    pub night: f32,
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
    app.init_resource::<Daylight>()
        .add_systems(Update, (turn, light).chain());
}

/// Eases the light towards the time of day of the latest view; the first
/// view of a session is lit as it is, without a turn.
fn turn(
    session: NonSend<Session>,
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
    let step = time.delta_secs() / TURN_SECONDS;
    daylight.night += (goal - daylight.night).clamp(-step, step);
}

/// Lays the light over the day look by how far it has turned.
fn light(
    daylight: Res<Daylight>,
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
    let moonlight = Color::srgb(0.55, 0.66, 1.0).to_linear();
    let blue = Color::srgb(0.42, 0.52, 1.0).to_linear();
    let tint = night * crate::look("night_tint", 0.6);
    sun.illuminance = day.sun * (1.0 - night * (1.0 - crate::look("night_sun", 0.3)));
    sun.color = day.sun_color.mix(&moonlight, tint).into();
    ambient.brightness = day.ambient * (1.0 - night * (1.0 - crate::look("night_ambient", 0.55)));
    ambient.color = day.ambient_color.mix(&blue, tint).into();
}
