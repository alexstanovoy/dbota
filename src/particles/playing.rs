//! Game permission and witnessed flights for the independent particle simulation.

use std::{collections::HashMap, sync::Arc};

use bevy::{camera::primitives::Frustum, prelude::*};
use dbota_particles::{Effect, Handle, Instance, Library, ParticleFrame};

/// What authorizes a world effect, rechecked for cards, lights, models and ongoing tails.
#[derive(Clone, Copy, Debug)]
pub enum EffectSight {
    Public,
    Point,
    KnownPoint(bota_proto::SeenBy),
    Witnessed(bota_proto::SeenBy),
    FlightEnd {
        seen: bota_proto::SeenBy,
        tip: Vec2,
        trail_origin: Option<Vec2>,
    },
    Unit(bota_proto::EntityId),
    Projectile(bota_proto::EntityId),
}

/// The last row actually presented, retained across snapshots and visible flight endings.
#[derive(Clone, Copy)]
struct FlightRecord {
    id: bota_proto::EntityId,
    at: Vec2,
    seen: bota_proto::SeenBy,
    trail_origin: Option<Vec2>,
}

struct Presentation {
    sight: EffectSight,
    flight: Option<FlightRecord>,
}

/// The simulated effects and the game facts that permit presenting each generation.
#[derive(Resource, Default)]
pub struct Playing {
    simulation: dbota_particles::Simulation,
    presentation: HashMap<Handle, Presentation>,
    #[cfg(test)]
    hidden_effects: Vec<Arc<Effect>>,
    #[cfg(test)]
    isolated_effect: Option<Arc<Effect>>,
}

impl Playing {
    pub(super) fn reset(&mut self) {
        self.simulation.reset();
        self.presentation.clear();
        #[cfg(test)]
        {
            self.hidden_effects.clear();
            self.isolated_effect = None;
        }
    }

    pub fn start(&mut self, effect: &Arc<Effect>, seed: u32, sight: EffectSight) -> Option<Handle> {
        let handle = self.simulation.start(effect, seed)?;
        self.presentation.insert(
            handle,
            Presentation {
                sight,
                flight: None,
            },
        );
        Some(handle)
    }

    pub fn hold(&mut self, effect: &Arc<Effect>, seed: u32, sight: EffectSight) -> Option<Handle> {
        let handle = self.start(effect, seed, sight)?;
        self.get_mut(handle)?.hold();
        Some(handle)
    }

    pub fn hold_on_stage(&mut self, effect: &Arc<Effect>, seed: u32) -> Option<Handle> {
        let handle = self.simulation.hold_on_stage(effect, seed)?;
        self.presentation.insert(
            handle,
            Presentation {
                sight: EffectSight::Public,
                flight: None,
            },
        );
        Some(handle)
    }

    pub fn get_mut(&mut self, handle: Handle) -> Option<&mut Instance> {
        self.simulation.get_mut(handle)
    }

    /// Updates a live generation's permission before changing its simulation state.
    pub fn get_mut_with_sight(
        &mut self,
        handle: Handle,
        sight: EffectSight,
    ) -> Option<&mut Instance> {
        let instance = self.simulation.get_mut(handle)?;
        self.presentation.get_mut(&handle)?.sight = sight;
        Some(instance)
    }

    pub fn started(&self, effect: &Arc<Effect>) -> u32 {
        self.simulation.started(effect)
    }

    pub fn note_flight(
        &mut self,
        handle: Handle,
        projectile: &bota_proto::ProjectileView,
        seen: bota_proto::SeenBy,
        trail_origin: Option<Vec2>,
    ) {
        if self.simulation.contains(handle)
            && let Some(presentation) = self.presentation.get_mut(&handle)
        {
            presentation.flight = Some(FlightRecord {
                id: projectile.id,
                at: Vec2::new(projectile.pos.x.to_f32(), projectile.pos.y.to_f32()),
                seen,
                trail_origin,
            });
        }
    }

    /// A visible head without its witnessed tether cannot keep an older trail.
    pub fn clear_flight_observation(&mut self, handle: Handle) {
        if let Some(presentation) = self.presentation.get_mut(&handle) {
            presentation.flight = None;
        }
    }

    pub fn witnessed_flight(&self, handle: Handle, sight: &crate::fog::EventSight) -> bool {
        self.simulation.contains(handle)
            && self
                .presentation
                .get(&handle)
                .and_then(|presentation| presentation.flight)
                .is_some_and(|flight| !sight.fog().hides(Some(flight.seen)))
    }

    /// Retires a hidden flight without revealing its impact or end cap.
    pub fn cancel(&mut self, handle: Handle) {
        self.simulation.cancel(handle);
        self.presentation.remove(&handle);
    }

    /// A visible ending keeps its witnessed point, without querying a hidden live target.
    pub fn finish_flight(&mut self, handle: Handle, sight: &crate::fog::EventSight) {
        let last = self.presentation.get(&handle).and_then(|presentation| {
            let flight = presentation.flight?;
            (!sight.has_projectile(flight.id)
                && allows_ending(sight, flight.seen, flight.at, flight.trail_origin))
            .then_some(EffectSight::FlightEnd {
                seen: flight.seen,
                tip: flight.at,
                trail_origin: flight.trail_origin,
            })
        });
        match last {
            Some(ending) => {
                if let (Some(instance), Some(presentation)) = (
                    self.simulation.get_mut(handle),
                    self.presentation.get_mut(&handle),
                ) {
                    instance.end();
                    presentation.sight = ending;
                }
            }
            None => self.cancel(handle),
        }
    }

    pub(super) fn step(
        &mut self,
        dt: f32,
        height: Option<dbota_particles::HeightAt<'_>>,
        library: Option<&Library>,
        camera: Option<(&GlobalTransform, &Frustum)>,
        stage: Option<dbota_particles::StageView<'_>>,
        frame: &mut ParticleFrame,
        units_per_metre: f32,
        sight: &crate::fog::EventSight,
    ) {
        let Self {
            simulation,
            presentation,
            #[cfg(test)]
            hidden_effects,
            #[cfg(test)]
            isolated_effect,
        } = self;
        #[cfg(test)]
        simulation.record_draws();
        simulation.step(
            dt,
            height,
            library,
            camera,
            stage,
            frame,
            units_per_metre,
            &|handle, instance| {
                #[cfg(test)]
                if isolated_effect
                    .as_ref()
                    .is_some_and(|effect| !Arc::ptr_eq(effect, &instance.effect))
                    || hidden_effects
                        .iter()
                        .any(|effect| Arc::ptr_eq(effect, &instance.effect))
                {
                    return false;
                }
                presentation
                    .get(&handle)
                    .is_some_and(|permission| permitted(permission.sight, instance, sight))
            },
        );
        presentation.retain(|handle, _| simulation.contains(*handle));
    }

    /// Measures one effect's submitted geometry when other effects share the same draw batch.
    #[cfg(test)]
    pub fn isolate_effect(&mut self, effect: Arc<Effect>) {
        self.isolated_effect = Some(effect);
    }

    /// Masks presentation only: simulation, game state and all nonselected effects stay intact.
    #[cfg(test)]
    pub fn hide_effects(&mut self, effects: Vec<Arc<Effect>>) {
        assert!(effects.len() <= 16, "a bounded render comparison");
        self.hidden_effects = effects;
    }

    /// Masks presentation only: leaves out the systems of `effect` whose path contains
    /// `matching`, and none when it is empty.
    #[cfg(test)]
    pub fn hide_systems(&mut self, effect: &Arc<Effect>, matching: &str) {
        self.simulation.hide_systems(effect, matching);
    }

    #[cfg(test)]
    pub fn advance(
        &mut self,
        dt: f32,
        stage_dt: Option<f32>,
        ground: Option<dbota_particles::HeightAt<'_>>,
    ) {
        self.simulation.advance(dt, stage_dt, ground);
        self.presentation
            .retain(|handle, _| self.simulation.contains(*handle));
    }

    #[cfg(test)]
    pub fn drawn(&self, effect: &Arc<Effect>) -> u32 {
        self.simulation.drawn(effect)
    }

    #[cfg(test)]
    pub fn drawn_endings(&self, effect: &Arc<Effect>) -> u32 {
        self.simulation.drawn_endings(effect)
    }

    #[cfg(test)]
    pub fn endings_this_frame(&self, effect: &Arc<Effect>) -> u32 {
        self.simulation.endings_this_frame(effect)
    }

    #[cfg(test)]
    pub fn instances_of<'a>(
        &'a self,
        effect: &'a Arc<Effect>,
    ) -> impl Iterator<Item = InstanceRef<'a>> {
        self.simulation
            .instances()
            .filter(move |(_, instance)| Arc::ptr_eq(&instance.effect, effect))
            .map(|(handle, instance)| InstanceRef {
                instance,
                flight: self
                    .presentation
                    .get(&handle)
                    .and_then(|permission| permission.flight),
            })
    }
}

#[cfg(test)]
pub struct InstanceRef<'a> {
    instance: &'a Instance,
    flight: Option<FlightRecord>,
}

#[cfg(test)]
impl std::ops::Deref for InstanceRef<'_> {
    type Target = Instance;
    fn deref(&self) -> &Self::Target {
        self.instance
    }
}

#[cfg(test)]
impl InstanceRef<'_> {
    pub fn flight_id(&self) -> Option<bota_proto::EntityId> {
        self.flight.map(|flight| flight.id)
    }
}

fn allows_ending(
    sight: &crate::fog::EventSight,
    seen: bota_proto::SeenBy,
    tip: Vec2,
    trail_origin: Option<Vec2>,
) -> bool {
    !sight.fog().hides(Some(seen))
        && sight.allows_point(tip.x, tip.y)
        && trail_origin.is_none_or(|at| sight.allows_point(at.x, at.y))
}

fn permitted(permission: EffectSight, instance: &Instance, sight: &crate::fog::EventSight) -> bool {
    match permission {
        EffectSight::Public => true,
        EffectSight::Unit(id) => sight.visible_unit(id).is_some(),
        EffectSight::Projectile(id) => sight.sees_projectile(id),
        EffectSight::Witnessed(seen) => !sight.fog().hides(Some(seen)),
        EffectSight::FlightEnd {
            seen,
            tip,
            trail_origin,
        } => allows_ending(sight, seen, tip, trail_origin),
        EffectSight::KnownPoint(seen) if sight.fog().hides(Some(seen)) => false,
        EffectSight::Point | EffectSight::KnownPoint(_) => {
            instance.nodes.first().is_some_and(|node| {
                let at = node.control_points[0].position;
                sight.allows_point(at.x, at.y)
            })
        }
    }
}
