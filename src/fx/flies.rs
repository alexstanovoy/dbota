//! Particle bindings for the visible heads and spans of travelling projectiles.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::ecs::system::SystemParam;
use bevy::{prelude::*, transform::TransformSystems};
use bota_client_core::{Flight, radians};
use bota_proto::{EntityId, Team, TetherHead, TetherView};
use dbota_particles::Effect;
use heatel::particle::{FloatInput, Operator, System};

use super::line::{At, Attach, Trigger};
use super::rules::{Rule, Rules};
use super::{Feeds, attach_point, feed, place_of};
use crate::effects::{MISSILE_HEIGHT, Shooters, steer};
use crate::fog::EventSight;
use crate::manifest::Manifest;
use crate::map::Ground;
use crate::particles::{EffectSight, Handle, Library, Playing};
use crate::units::{ModelBank, Unit, UnitIndex, Wearable};

const LAUNCH: usize = 0;
const TIP: usize = 1;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    rule: usize,
    projectile: EntityId,
    span: Option<usize>,
}

#[derive(Resource, Default)]
struct Flying {
    held: HashMap<Key, (Handle, u32)>,
    endings: Vec<Handle>,
    frame: u32,
    tick: u32,
    viewer: Option<Team>,
}

impl Flying {
    fn begin(&mut self, tick: u32, viewer: Option<Team>, playing: &mut Playing) {
        if tick < self.tick || self.viewer != viewer {
            for (_, (handle, _)) in self.held.drain() {
                playing.cancel(handle);
            }
            for handle in self.endings.drain(..) {
                playing.cancel(handle);
            }
        }
        self.endings
            .retain(|handle| playing.get_mut(*handle).is_some());
        self.tick = tick;
        self.viewer = viewer;
        self.frame = self.frame.wrapping_add(1);
    }

    fn hold(
        &mut self,
        key: Key,
        effect: &Arc<Effect>,
        sight: EffectSight,
        playing: &mut Playing,
    ) -> Option<Handle> {
        if let Some((handle, frame)) = self.held.get_mut(&key) {
            *frame = self.frame;
            if playing.get_mut_with_sight(*handle, sight).is_some() {
                return Some(*handle);
            }
        }
        let handle = playing.hold(
            effect,
            key.projectile.idx ^ key.projectile.generation,
            sight,
        )?;
        self.held.insert(key, (handle, self.frame));
        Some(handle)
    }

    fn finish(&mut self, sight: &EventSight, playing: &mut Playing, rules: &Rules) {
        self.held.retain(|key, (handle, frame)| {
            if *frame == self.frame {
                return true;
            }
            let tethered = rules
                .get(key.rule)
                .is_some_and(|rule| rule.fx.at == At::Chain);
            let still_visible = tethered
                && sight.core().view().is_some_and(|view| {
                    view.tethers.iter().any(|tether| {
                        tether.id == key.projectile
                            && self.viewer.is_none_or(|team| tether.viewer == team)
                    })
                });
            if key.span.is_some() || still_visible {
                // Clipped spans and heads can disappear as vision changes.
                playing.cancel(*handle);
                return false;
            }
            playing.finish_flight(*handle, sight);
            if tethered && let Some(instance) = playing.get_mut(*handle) {
                // Detach the trail from the hand and retire the flight model.
                instance.hide_systems(|def| {
                    !def.model_draws.is_empty() || maintains_chain(&def.system)
                });
                self.endings.push(*handle);
            }
            false
        });
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Flying>()
        .add_systems(
            First,
            crate::loading::reset::<Flying>.in_set(crate::loading::Begin),
        )
        .add_systems(
            PostUpdate,
            flies
                .after(TransformSystems::Propagate)
                .before(crate::particles::Simulation),
        )
        .add_systems(
            PostUpdate,
            stow_wearables.before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
}

/// A visible source puts away the wearable named by its active travelling effect.
fn stow_wearables(
    sight: EventSight,
    manifest: Option<Res<Manifest>>,
    rules: Res<Rules>,
    mut wearables: Query<(&Wearable, &mut Visibility)>,
) {
    let view = sight.core().view();
    let audience = view.and_then(|view| view.viewer).or(sight.fog().team);
    for (wearable, mut shown) in &mut wearables {
        let Some((unit, path)) = wearable.world_model() else {
            continue;
        };
        let hidden = manifest.as_deref().is_some_and(|manifest| {
            view.into_iter()
                .flat_map(|view| &view.tethers)
                .any(|tether| {
                    if audience.is_some_and(|team| team != tether.viewer) {
                        return false;
                    }
                    tether.source == Some(unit)
                        && sight.visible_unit(unit).is_some()
                        && rules.of(Trigger::Flies).any(|(_, rule)| {
                            rule.names(Some(tether.inflictor))
                                && rule
                                    .fx
                                    .hide_wearable
                                    .as_deref()
                                    .and_then(|name| manifest.model(name))
                                    .is_some_and(|art| {
                                        path == bevy::asset::AssetPath::parse(&art.path).path()
                                    })
                        })
                })
        });
        shown.set_if_neq(if hidden {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

fn above_ground(ground: &Ground, at: Vec2, height: f32) -> Vec3 {
    Vec3::new(at.x, ground.at(at.x, at.y) + height, -at.y)
}

fn point(at: bota_proto::Vec2) -> Vec2 {
    Vec2::new(at.x.to_f32(), at.y.to_f32())
}

fn heading(angle: bota_proto::Angle) -> Vec3 {
    let (sin, cos) = radians(angle).sin_cos();
    Vec3::new(cos, 0.0, -sin)
}

/// Place the head at its launch socket until it clears the socket's reach. The return
/// destination is fixed by the throw, so a moving hand only moves the chain's near end.
fn blend_from_socket(centre: Vec3, hand: Vec3, tip: Vec3) -> Vec3 {
    let offset = hand - centre;
    let reach = offset.length();
    if reach <= f32::EPSILON {
        return tip;
    }
    tip + offset * (1.0 - (tip.distance(centre) / reach).min(1.0))
}

/// A directly placed chain tip uses the ground offset authored for its root particle.
fn chain_height(effect: &Effect) -> f32 {
    effect.defs[0]
        .system
        .operators
        .iter()
        .find_map(|operator| {
            let Operator::MovementPlaceOnGround(movement) = &operator.class else {
                return None;
            };
            match movement.offset {
                FloatInput::Literal(height) if !movement.scale_offset => Some(height),
                _ => None,
            }
        })
        .unwrap_or(MISSILE_HEIGHT)
}

#[derive(SystemParam)]
struct FlightPresentation<'w, 's> {
    ground: Res<'w, Ground>,
    playing: ResMut<'w, Playing>,
    flying: ResMut<'w, Flying>,
    bodies: Shooters<'w, 's>,
    units: Query<'w, 's, &'static Unit>,
    unit_index: Res<'w, UnitIndex>,
    bank: Res<'w, ModelBank>,
    manifest: Option<Res<'w, Manifest>>,
}

fn flies(
    sight: EventSight,
    mut presentation: FlightPresentation,
    rules: Res<Rules>,
    library: Option<Res<Library>>,
) {
    let Some(view) = sight.core().view() else {
        return;
    };
    let team = view.viewer.or(sight.fog().team);
    presentation
        .flying
        .begin(view.tick, team, &mut presentation.playing);
    for (index, rule) in rules.of(Trigger::Flies) {
        let Some(effect) = rule
            .fx
            .particle
            .as_deref()
            .and_then(|name| library.as_deref()?.effect(name))
        else {
            continue;
        };
        if rule.fx.at == At::Chain {
            let height = chain_height(effect);
            for tether in view.tethers.iter().filter(|tether| {
                rule.names(Some(tether.inflictor))
                    && team.map_or_else(
                        || {
                            view.projectile(tether.id)
                                .is_some_and(|projectile| projectile.team == tether.viewer)
                        },
                        |team| tether.viewer == team,
                    )
            }) {
                presentation.tether(
                    &sight,
                    index,
                    rule,
                    effect,
                    height,
                    tether,
                    library.as_deref().expect("the effect library"),
                );
            }
        } else {
            for flight in sight.flights() {
                if rule.names(flight.missile.inflictor) {
                    presentation.missile(&sight, index, rule, effect, &flight);
                }
            }
        }
    }
    presentation
        .flying
        .finish(&sight, &mut presentation.playing, &rules);
}

impl FlightPresentation<'_, '_> {
    fn missile(
        &mut self,
        sight: &EventSight,
        index: usize,
        rule: &Rule,
        effect: &Arc<Effect>,
        flight: &Flight,
    ) {
        let key = Key {
            rule: index,
            projectile: flight.missile.id,
            span: None,
        };
        let Some(handle) = self.flying.hold(
            key,
            effect,
            EffectSight::Projectile(key.projectile),
            &mut self.playing,
        ) else {
            return;
        };
        self.playing.note_flight(
            handle,
            &flight.missile,
            sight.knowledge(flight.missile.seen_by),
            None,
        );
        let tip = above_ground(&self.ground, Vec2::new(flight.x, flight.y), MISSILE_HEIGHT);
        let delta = Vec3::new(flight.x - flight.tail_x, 0.0, -(flight.y - flight.tail_y));
        let source = flight.missile.source.and_then(|id| sight.visible_unit(id));
        let hand = source
            .and_then(|unit| self.bodies.launch(unit.id))
            .unwrap_or(tip);
        let instance = self.playing.get_mut(handle).expect("a held effect");
        instance.set_control_point(LAUNCH, place_of(hand), Vec3::ZERO, false);
        steer(tip + delta, delta, &mut self.playing, handle);
        let caster =
            source.map(|unit| attach_point(&self.ground, &self.bodies, unit, &Attach::Origin));
        let feeds = Feeds {
            caster,
            flight_tip: tip,
            flight_speed: delta.length() * bota_core::rules::TICKS_PER_SECOND as f32,
            ..Feeds::default()
        };
        feed(
            &rule.fx,
            Some(handle),
            &mut self.playing,
            &feeds,
            &|attach| {
                source.map_or(tip, |unit| {
                    attach_point(&self.ground, &self.bodies, unit, attach)
                })
            },
        );
    }

    fn tether(
        &mut self,
        sight: &EventSight,
        index: usize,
        rule: &Rule,
        effect: &Arc<Effect>,
        height: f32,
        tether: &TetherView,
        library: &Library,
    ) {
        let source = tether.source.and_then(|id| sight.visible_unit(id));
        let hand = source
            .filter(|unit| sight.attachment_was_visible(unit.id))
            .map(|unit| attach_point(&self.ground, &self.bodies, unit, &rule.fx.attach));
        let caster = source
            .filter(|unit| sight.attachment_was_visible(unit.id))
            .map(|unit| attach_point(&self.ground, &self.bodies, unit, &Attach::Origin));
        let target = tether
            .head
            .and_then(|head| head.caught)
            .and_then(|id| sight.visible_unit(id))
            .filter(|unit| unit.hp > 0);
        let caught = target.is_some();
        let goal = target
            .filter(|unit| sight.attachment_was_visible(unit.id))
            .zip(rule.fx.target_attach.as_ref())
            .map(|(unit, attach)| attach_point(&self.ground, &self.bodies, unit, attach));
        let motion = tether
            .head
            .and_then(|head| self.tether_head(sight, tether, head, height, hand, goal));
        let whole = hand.is_some()
            && tether.spans.len() == 1
            && tether.spans[0].reaches_source
            && tether.spans[0].reaches_head;
        let scale = source
            .map(|unit| self.bodies.object_scale(unit.id))
            .or_else(|| {
                let manifest = self.manifest.as_deref()?;
                let wearable = manifest.model(rule.fx.hide_wearable.as_deref()?)?;
                Some(manifest.model(wearable.attach.as_deref()?)?.scale)
            });
        let mut visible_head = None;
        if let Some((tip, direction, velocity, speed)) = motion {
            let launch = hand.unwrap_or(tip);
            let key = Key {
                rule: index,
                projectile: tether.id,
                span: None,
            };
            let position = TetherPlacement {
                launch,
                tip,
                direction,
                velocity,
                speed,
                caster,
                scale,
                caught,
            };
            let draws = if whole {
                TetherDraws::All
            } else {
                TetherDraws::Head
            };
            visible_head = self.place_tether(rule, effect, key, position, draws);
            if let Some(handle) = visible_head {
                if whole
                    && tether
                        .head
                        .is_some_and(|head| head.returning && head.origin.is_some())
                    && let Some(projectile) = sight
                        .core()
                        .view()
                        .and_then(|view| view.projectile(tether.id))
                {
                    self.playing.note_flight(
                        handle,
                        projectile,
                        sight.knowledge(projectile.seen_by),
                        Some(place_of(launch).truncate()),
                    );
                } else {
                    self.playing.clear_flight_observation(handle);
                }
                let model = source
                    .and_then(|unit| self.unit_index.0.get(&unit.id))
                    .and_then(|entity| self.units.get(*entity).ok())
                    .and_then(|unit| unit.model)
                    .and_then(|model| self.bank.0.get(model))
                    .map(|entry| &entry.model);
                let manifest = self.manifest.as_deref();
                let instance = self.playing.get_mut(handle).expect("a held effect");
                instance.bind_models(library, |point, slot| {
                    if !rule
                        .fx
                        .feeds
                        .iter()
                        .any(|(cp, feed)| *cp == point && matches!(feed, super::line::Feed::Caster))
                    {
                        return None;
                    }
                    let model = model?;
                    let art = match slot {
                        None => model,
                        Some(slot) => manifest?.models.iter().find(|worn| {
                            worn.attach.as_deref() == Some(model.logical.as_str())
                                && worn.logical.rsplit('.').next() == Some(slot)
                        })?,
                    };
                    art.source.strip_suffix("_c")
                });
            }
        }
        if whole && motion.is_some() {
            return;
        }
        for (span, segment) in tether.spans.iter().enumerate() {
            let first = above_ground(&self.ground, point(segment.start), height);
            let last = above_ground(&self.ground, point(segment.end), height);
            let launch = hand.filter(|_| segment.reaches_source).unwrap_or(first);
            let tip = motion
                .filter(|_| segment.reaches_head)
                .map_or(last, |(tip, ..)| tip);
            let direction = (last - first).normalize_or_zero();
            let key = Key {
                rule: index,
                projectile: tether.id,
                span: Some(span),
            };
            let position = TetherPlacement {
                launch,
                tip,
                direction,
                velocity: direction,
                speed: 0.0,
                caster,
                scale,
                caught: false,
            };
            if let Some(handle) = self.place_tether(rule, effect, key, position, TetherDraws::Chain)
                && segment.reaches_head
                && let Some(head) = visible_head
                && let Some(point) = tip_point(rule)
            {
                self.playing
                    .get_mut(handle)
                    .expect("a held rope")
                    .attach_rope_to_flight(point, head);
            }
        }
    }

    fn tether_head(
        &self,
        sight: &EventSight,
        tether: &TetherView,
        head: TetherHead,
        height: f32,
        hand: Option<Vec3>,
        target: Option<Vec3>,
    ) -> Option<(Vec3, Vec3, Vec3, f32)> {
        let view = sight.core().view()?;
        let projectile = view.projectile(tether.id)?;
        if !sight.allows_projectile(projectile) {
            return None;
        }
        let previous = sight.previous_projectile(tether.id);
        let flight =
            bota_client_core::interpolate_flight(projectile, previous, sight.core().smooth.alpha());
        let mut tip = above_ground(&self.ground, Vec2::new(flight.x, flight.y), height);
        if let (Some(home), Some(hand), Some(source)) = (head.origin, hand, tether.source)
            && sight
                .visible_unit(source)
                .is_some_and(|unit| unit.pos == home)
        {
            tip = blend_from_socket(above_ground(&self.ground, point(home), height), hand, tip);
        }
        Some((
            target.unwrap_or(tip),
            heading(head.cast_direction),
            heading(projectile.facing),
            head.speed.to_f32(),
        ))
    }

    fn place_tether(
        &mut self,
        rule: &Rule,
        effect: &Arc<Effect>,
        key: Key,
        position: TetherPlacement,
        draws: TetherDraws,
    ) -> Option<Handle> {
        let permission = if key.span.is_some() {
            EffectSight::Public
        } else {
            EffectSight::Projectile(key.projectile)
        };
        let handle = self
            .flying
            .hold(key, effect, permission, &mut self.playing)?;
        let TetherPlacement {
            launch,
            tip,
            direction,
            velocity,
            speed,
            caster,
            scale,
            caught,
        } = position;
        let instance = self.playing.get_mut(handle).expect("a held effect");
        instance.show_systems(|def| {
            let chain = maintains_chain(&def.system);
            match draws {
                TetherDraws::All => true,
                TetherDraws::Head => !chain,
                TetherDraws::Chain => chain,
            }
        });
        instance.set_control_point(LAUNCH, place_of(launch), place_of(direction), false);
        instance.set_direction_from(LAUNCH, place_of(direction));
        instance.set_control_point(TIP, place_of(tip), place_of(velocity), false);
        instance.drive_carrier(place_of(tip), place_of(velocity));
        let attachment = matches!(draws, TetherDraws::All)
            .then(|| tip_point(rule))
            .flatten();
        instance.attach_rope_to_model(attachment);
        let flight_lifetime = 2.0 * bota_core::rules::HOOK_RANGE as f32 / speed.max(1.0);
        let feeds = Feeds {
            caster,
            caster_scale: scale,
            flight_tip: tip,
            flight_speed: speed,
            flight_lifetime,
            flight_caught: caught,
            ..Feeds::default()
        };
        feed(&rule.fx, Some(handle), &mut self.playing, &feeds, &|_| {
            launch
        });
        Some(handle)
    }
}

fn maintains_chain(system: &System) -> bool {
    system
        .operators
        .iter()
        .any(|operator| matches!(operator.class, Operator::MaintainSequentialPath(_)))
}

fn tip_point(rule: &Rule) -> Option<usize> {
    rule.fx
        .feeds
        .iter()
        .find_map(|(point, feed)| matches!(feed, super::line::Feed::FlightTip).then_some(*point))
}

#[derive(Clone, Copy)]
enum TetherDraws {
    All,
    Head,
    Chain,
}

struct TetherPlacement {
    launch: Vec3,
    tip: Vec3,
    direction: Vec3,
    velocity: Vec3,
    speed: f32,
    caster: Option<Vec3>,
    scale: Option<f32>,
    caught: bool,
}
