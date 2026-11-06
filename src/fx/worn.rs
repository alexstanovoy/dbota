//! The lines of what units wear: an effect a modifier, an ability or an item put on one, or a unit
//! of a Dota name standing; and of the runes lying in sight. From the moment a unit shows it until
//! it goes, its particle follows the unit and is fed what the line asks; its sound is made once.

use std::collections::{HashMap, HashSet};

use bevy::{prelude::*, transform::TransformSystems};
use bota_core::rules::TICKS_PER_SECOND;
use bota_proto::{EntityId, Team, UnitView};

use super::line::{Attach, Fx, Trigger, Whose};
use super::rules::{Rule, Rules, Subject};
use super::{
    Feeds, PLACE, Stage, attach_basis, attach_point, carried, feed, item_feeds, nodes_ready,
    on_ground, place_of,
};
use crate::effects::Shooters;
use crate::map::Ground;
use crate::mixer::{Owner, Started};
use crate::particles::{EffectSight, Handle, Library, Playing};
use crate::runes;
use crate::units::{Dying, UnitIndex, held_for};

/// The worn effects playing: by the wearing unit and the rule, the particle if the line has one
/// and the loop of its sound, which sounds for as long as the effect is worn.
#[derive(Resource, Default)]
struct Worn(HashMap<(EntityId, usize), Wearing>);

/// What one worn effect plays.
#[derive(Clone, Copy)]
struct Wearing {
    particle: Option<Handle>,
    /// The voice of a sound that loops: owned by the wearing, and stopped when it ends. A sound
    /// that plays once plays out by itself, whatever becomes of the wearing.
    voice: Option<Entity>,
    /// Whether the sound loops but has not started yet (it was out of earshot): tried again
    /// while the effect is worn.
    waiting: bool,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Worn>().add_systems(
        PostUpdate,
        worn.after(TransformSystems::Propagate)
            .before(crate::particles::Simulation),
    );
}

/// What a unit holds of a rule's subject.
struct Hold {
    feeds: Feeds,
}

/// What the frame tells of the units beyond their own views: the map objectives heroes channel
/// at.
struct Facts {
    channelled: HashSet<EntityId>,
}

/// What `unit` holds of the rule's subject, if it wears it, or carries it, as the line asks.
fn holding(rule: &Rule, unit: &UnitView, facts: &Facts) -> Option<Hold> {
    let fx = &rule.fx;
    let side = held_for(unit);
    if fx.side.is_some_and(|wanted| side != Some(wanted)) {
        return None;
    }
    let side = match side {
        Some(Team::Dire) => 1.0,
        _ => 0.0,
    };
    let held = |stacks, ticks_left: Option<u32>, feeds| Hold {
        feeds: Feeds {
            stacks: stacks as f32,
            seconds: ticks_left.map_or(0.0, |ticks| ticks as f32 / TICKS_PER_SECOND as f32),
            side,
            ..feeds
        },
    };
    match (&rule.subject, fx.trigger) {
        (Subject::Units(units), Trigger::Channelled) => (units.is(unit)
            && facts.channelled.contains(&unit.id))
        .then(|| held(0, None, Feeds::default())),
        (Subject::Units(units), _) => units.is(unit).then(|| held(0, None, Feeds::default())),
        (Subject::Abilities(ids), Trigger::Casting) => unit
            .cast
            .and_then(|cast| cast.ability)
            .is_some_and(|ability| ids.contains(&ability))
            .then(|| held(0, None, Feeds::default())),
        (Subject::Items(ids), Trigger::Held) => ids.iter().find_map(|id| {
            let item = carried(Some(unit), *id)?;
            let (mode, charges) = item_feeds(Some(item));
            Some(held(
                0,
                None,
                Feeds {
                    mode,
                    charges,
                    ..Feeds::default()
                },
            ))
        }),
        _ => unit.effects.iter().find_map(|effect| {
            let on = match &rule.subject {
                Subject::Effects(ids) => ids.contains(&effect.id),
                _ => rule.names(effect.inflictor),
            };
            let stacks = effect.stacks.unwrap_or(0);
            let own = effect.caster == Some(unit.id);
            let whose = match fx.whose {
                Whose::Any => true,
                Whose::Own => own,
                Whose::Foreign => !own,
            };
            let fits = on && whose && (fx.stacks.0..=fx.stacks.1).contains(&stacks);
            fits.then(|| held(stacks, effect.ticks_left, Feeds::default()))
        }),
    }
}

/// Starts what a unit or a rune (`id`) that has come to wear a rule's subject wears: its
/// particle, held on until it is ended, and its sound, which a loop keeps until the wearing ends.
fn begin(
    fx: &Fx,
    id: EntityId,
    at: Vec3,
    (library, playing): (Option<&Library>, &mut Playing),
    stage: &mut Stage,
    sight: EffectSight,
) -> Wearing {
    let seed = id.idx ^ id.generation.rotate_left(16);
    let handle = fx
        .particle
        .as_deref()
        .and_then(|name| library?.effect(name))
        .and_then(|effect| playing.hold(effect, seed, sight));
    let mut wearing = Wearing {
        particle: handle,
        voice: None,
        waiting: fx.sound.is_some(),
    };
    sound(fx, id, at, &mut wearing, stage);
    wearing
}

/// Sounds a wearing's sound until it has started (or plays out): a loop out of earshot is tried
/// again every frame, so it starts late, when the camera comes near or the mixer has room, and
/// not from the moment the effect began; a loop sounding follows its wearer. A sound that plays
/// once and was refused is not tried again: its moment has passed.
fn sound(fx: &Fx, id: EntityId, at: Vec3, wearing: &mut Wearing, stage: &mut Stage) {
    let Some(prefix) = fx.sound.as_deref() else {
        return;
    };
    if let Some(voice) = wearing.voice {
        stage.follow(voice, at);
        return;
    }
    if !wearing.waiting {
        return;
    }
    let seed = id.idx ^ id.generation.rotate_left(16);
    match stage.sound(prefix, at, seed, Owner::Unit(id), Some(id)) {
        Started::Loop(voice) => {
            wearing.voice = Some(voice);
            wearing.waiting = false;
        }
        Started::Once => wearing.waiting = false,
        // Out of earshot or blocked: a one-shot is lost, a loop is tried again.
        Started::Refused => wearing.waiting = stage.loops(prefix),
    }
}

/// Ends what a wearing played: its particle lives out its life, its loop fades out.
fn end(wearing: &Wearing, playing: &mut Playing, stage: &mut Stage) {
    if let Some(instance) = wearing.particle.and_then(|h| playing.get_mut(h)) {
        instance.end();
    }
    if let Some(voice) = wearing.voice {
        stage.silence(voice);
    }
}

/// Starts what each unit that has come to wear a rule's subject wears, and each rune that lies in
/// sight, keeps its particle at the unit or the rune and fed, and ends it when the unit stops
/// wearing it, falls or leaves the view, or the rune is taken or leaves the sight. With no match
/// in view nothing stands: every wearing ends.
#[allow(clippy::too_many_arguments)]
fn worn(
    sight: crate::fog::EventSight,
    channels: Res<crate::anim::Channels>,
    index: Res<UnitIndex>,
    dying: Query<(), With<Dying>>,
    ground: Res<Ground>,
    rules: Res<Rules>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut worn: ResMut<Worn>,
    bodies: Shooters,
    mut stage: Stage,
) {
    let Some(view) = sight.core().view() else {
        for (_, wearing) in worn.0.drain() {
            end(&wearing, &mut playing, &mut stage);
        }
        stage.silence_loops();
        return;
    };
    let standing = |id: &EntityId| {
        sight.visible_unit(*id).is_some()
            && index
                .0
                .get(id)
                .is_some_and(|entity| !dying.contains(*entity))
    };
    let wearing = rules
        .of(Trigger::Worn)
        .chain(rules.of(Trigger::Held))
        .chain(rules.of(Trigger::Casting))
        .chain(rules.of(Trigger::Channelled))
        .collect::<Vec<_>>();
    if wearing.is_empty() {
        return;
    }
    let facts = Facts {
        channelled: channels.objectives(view),
    };
    worn.0.retain(|(id, rule), wearing| {
        let keep = rules.0.get(*rule).is_some_and(|rule| match rule.subject {
            Subject::Runes(kind) => {
                runes::shown(view, sight.fog()).any(|rune| rune.id == *id && rune.kind == kind)
            }
            _ => {
                standing(id)
                    && view
                        .unit(*id)
                        .is_some_and(|unit| holding(rule, unit, &facts).is_some())
            }
        });
        if !keep {
            end(wearing, &mut playing, &mut stage);
        }
        keep
    });
    let scene = Scene {
        facts: &facts,
        ground: &ground,
        bodies: &bodies,
        library: library.as_deref(),
    };
    for unit in view.units.iter().filter(|u| standing(&u.id)) {
        wear_unit(unit, &wearing, &scene, &mut worn, &mut playing, &mut stage);
    }
    wear_runes(
        view,
        sight.fog(),
        &wearing,
        &scene,
        &mut worn,
        &mut playing,
        &mut stage,
    );
}

fn wear_runes(
    view: &bota_proto::WorldView,
    fog: &crate::fog::FogGrid,
    wearing: &[(usize, &Rule)],
    scene: &Scene,
    worn: &mut Worn,
    playing: &mut Playing,
    stage: &mut Stage,
) {
    for rune in runes::shown(view, fog) {
        let at = on_ground(scene.ground, rune.pos);
        for (n, rule) in wearing {
            if rule.subject != Subject::Runes(rune.kind) {
                continue;
            }
            let handle = worn.0.entry((rune.id, *n)).or_insert_with(|| {
                let with = (scene.library, &mut *playing);
                begin(&rule.fx, rune.id, at, with, stage, EffectSight::Point)
            });
            sound(&rule.fx, rune.id, at, handle, stage);
            if let Some(instance) = handle.particle.and_then(|h| playing.get_mut(h)) {
                instance.set_control_point(PLACE, place_of(at), Vec3::ZERO, false);
            }
        }
    }
}

/// What the effects worn are placed with.
struct Scene<'a> {
    facts: &'a Facts,
    ground: &'a Ground,
    bodies: &'a Shooters<'a, 'a>,
    library: Option<&'a Library>,
}

/// Starts and keeps what one unit wears of each rule it holds.
fn wear_unit(
    unit: &UnitView,
    wearing: &[(usize, &Rule)],
    scene: &Scene,
    worn: &mut Worn,
    playing: &mut Playing,
    stage: &mut Stage,
) {
    for (n, rule) in wearing {
        let Some(held) = holding(rule, unit, scene.facts) else {
            continue;
        };
        let fx = &rule.fx;
        if !worn.0.contains_key(&(unit.id, *n)) && !nodes_ready(fx, scene.bodies, unit) {
            continue;
        }
        let at = attach_point(scene.ground, scene.bodies, unit, &fx.attach);
        let handle = worn.0.entry((unit.id, *n)).or_insert_with(|| {
            begin(
                fx,
                unit.id,
                at,
                (scene.library, &mut *playing),
                stage,
                EffectSight::Unit(unit.id),
            )
        });
        sound(fx, unit.id, at, handle, stage);
        if let Some(instance) = handle.particle.and_then(|h| playing.get_mut(h)) {
            instance.bind_control_point(
                PLACE,
                place_of(at),
                attach_basis(scene.bodies, unit, &fx.attach),
                scene.bodies.object_scale(unit.id),
            );
        }
        let place = |attach: &Attach| attach_point(scene.ground, scene.bodies, unit, attach);
        feed(fx, handle.particle, playing, &held.feeds, &place);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fog::FogGrid;
    use crate::session::Session;
    use bevy::asset::{
        AssetApp,
        io::{
            AssetSourceBuilder,
            memory::{Dir, MemoryAssetReader},
        },
    };
    use bevy::audio::{AudioLoader, AudioSink, PlaybackMode, Source};
    use bevy::ecs::system::RunSystemOnce;
    use clap::Parser;
    use std::time::Duration;

    fn casting_sound_app() -> (App, bevy::asset::Handle<AudioSource>) {
        let source = crate::mixer::two_minute_source();
        let decoder = crate::sound_file::decoder(source.clone()).expect("a valid synthetic WAV");
        assert_eq!(decoder.total_duration(), Some(Duration::from_secs(120)));
        let memory = Dir::default();
        memory.insert_asset(std::path::Path::new("cast.wav"), source.bytes.to_vec());
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .register_asset_source(
                crate::CACHE_SOURCE,
                AssetSourceBuilder::new(move || {
                    Box::new(MemoryAssetReader {
                        root: memory.clone(),
                    })
                }),
            )
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<AudioSource>()
            .init_asset_loader::<AudioLoader>()
            .init_resource::<Playing>()
            .init_resource::<super::super::Heard>()
            .init_resource::<crate::mixer::Mixer>()
            .init_resource::<FogGrid>()
            .insert_resource(crate::config::Settings::default())
            .insert_resource(crate::Args::parse_from(["dbota"]))
            .insert_resource(crate::manifest::Manifest {
                files: vec![("cast".to_owned(), "cast.wav".to_owned())],
                sound_events: vec![("cast.wav".to_owned(), "Synthetic.Cast".to_owned())],
                ..Default::default()
            })
            .insert_non_send(crate::main_menu::idle());
        app.world_mut().spawn(crate::camera::RtsCamera {
            focus: Vec3::ZERO,
            distance: 1000.0,
        });
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<AudioSource>(crate::manifest::cache_path("cast.wav"));
        app.world_mut()
            .resource_mut::<Assets<AudioSource>>()
            .insert(handle.id(), source)
            .expect("the reserved audio asset id");
        (app, handle)
    }

    fn policy_mixer(policy: &str) -> crate::mixer::Mixer {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "dbota-fx-policy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("sounds")).expect("fixture folder");
        std::fs::write(root.join("sounds/events.txt"), policy).expect("fixture event");
        let mixer = crate::mixer::Mixer::load(&crate::manifest::Manifest {
            root: root.clone(),
            ..Default::default()
        });
        std::fs::remove_dir_all(root).expect("the event was already read");
        mixer
    }

    #[test]
    fn an_own_emitters_one_shot_gets_its_player_factor_without_becoming_a_loop() {
        let (mut app, source) = casting_sound_app();
        let (core, emitters) = crate::units::tests::owned_hero_scene();
        app.insert_non_send(Session(core));
        app.insert_resource(policy_mixer(
            "event Synthetic.Cast 1 120 0 0 0 0 1 0 -1 1 70 10000 - 0.25 -1\n",
        ));
        let world = app.world_mut();
        let expected = world.resource::<crate::mixer::Mixer>().gain_for(
            "Synthetic.Cast",
            (0.0, 0.0),
            (0.0, 0.0),
        ) * 0.25;
        let fx =
            Fx::parse(&["ability.test.cast", "at=caster", "sound=cast"]).expect("a one-shot row");
        world
            .run_system_once(move |mut stage: Stage, mut playing: ResMut<Playing>| {
                stage.play(
                    &fx,
                    (Vec3::ZERO, 0),
                    (None, &mut playing),
                    (&Feeds::default(), &|_| Vec3::ZERO),
                    crate::particles::EffectSight::Point,
                    Some(emitters[0]),
                );
            })
            .expect("the actual one-shot FX producer");
        let (player, settings) = world
            .query::<(&AudioPlayer, &PlaybackSettings)>()
            .single(world)
            .expect("one physical player");
        assert_eq!(player.0.id(), source.id());
        assert!(
            world
                .resource::<Assets<AudioSource>>()
                .get(&source)
                .is_some()
        );
        assert!(
            matches!(settings.mode, PlaybackMode::Despawn),
            "emitter identity became loop ownership"
        );
        assert!(
            (settings.volume.to_linear() - expected).abs() < 0.000_01,
            "an own emitter sounded at {}, expected {expected}",
            settings.volume.to_linear()
        );
    }

    #[test]
    fn ending_a_cast_effect_leaves_its_one_shot_player_until_its_deadline() {
        let (mut app, source) = casting_sound_app();
        let world = app.world_mut();
        let fx = Fx::parse(&["ability.test.casting", "at=unit", "sound=cast"])
            .expect("a sound-only casting row");
        let wearing = world
            .run_system_once(move |mut stage: Stage, mut playing: ResMut<Playing>| {
                begin(
                    &fx,
                    EntityId {
                        idx: 1,
                        generation: 1,
                    },
                    Vec3::ZERO,
                    (None, &mut playing),
                    &mut stage,
                    EffectSight::Point,
                )
            })
            .expect("the real worn producer starts");
        let (player, settings) = world
            .query::<(&AudioPlayer, &PlaybackSettings)>()
            .single(world)
            .expect("one physical player");
        assert!(matches!(settings.mode, PlaybackMode::Despawn));
        assert_eq!(player.0.id(), source.id());
        assert!(
            world
                .resource::<Assets<AudioSource>>()
                .get(&player.0)
                .is_some(),
            "the real WAV is resident"
        );
        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(2));
        world
            .run_system_once(move |mut stage: Stage, mut playing: ResMut<Playing>| {
                end(&wearing, &mut playing, &mut stage);
            })
            .expect("the real worn producer ends");
        assert_eq!(
            world.query::<&AudioPlayer>().iter(world).count(),
            1,
            "owner completion cut the one-shot"
        );
        world
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(8));
        world
            .run_system_once(
                |mut commands: Commands,
                 mut mixer: ResMut<crate::mixer::Mixer>,
                 time: Res<Time>,
                 mut sinks: Query<&mut AudioSink>| {
                    mixer.tick(&mut commands, time.elapsed(), &mut sinks, None);
                },
            )
            .expect("the playback deadline is applied");
        assert_eq!(
            world.query::<&AudioPlayer>().iter(world).count(),
            0,
            "the bounded deadline left a physical player"
        );
    }
}
