//! Which facts of a full stream the selected vision may present.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bota_client_core::App as Core;
use bota_proto::{EntityId, EventKind, UnitView, Vec2, WorldView};

use super::FogGrid;
use crate::session::Session;

/// A fact's subject, or the particular placement an effect draws it at.
#[derive(Clone, Copy)]
pub(crate) enum EventSubject<'a> {
    Event,
    Unit(&'a UnitView),
    Point(Vec2),
}

/// The same event audience for effects, sounds and interface announcements.
#[derive(SystemParam)]
pub(crate) struct EventSight<'w> {
    session: NonSend<'w, Session>,
    fog: Res<'w, FogGrid>,
    projectiles: Res<'w, super::ProjectileSight>,
    epoch: Res<'w, crate::main_menu::GameEpoch>,
}

impl EventSight<'_> {
    pub fn core(&self) -> &Core {
        &self.session.0
    }

    pub fn fog(&self) -> &FogGrid {
        &self.fog
    }

    fn unit(&self, id: EntityId) -> Option<&UnitView> {
        self.core()
            .view()?
            .unit(id)
            .or_else(|| self.core().prev_view()?.unit(id))
            .or_else(|| self.core().known(id).map(|(unit, _)| unit))
    }

    fn sees_unit(&self, id: EntityId) -> bool {
        self.core()
            .view()
            .and_then(|view| view.unit(id))
            .is_some_and(|unit| self.allows_unit(unit))
    }

    fn sees_point(&self, at: Vec2) -> bool {
        self.sees_point_xy(at.x.to_f32(), at.y.to_f32())
    }

    fn sees_point_xy(&self, x: f32, y: f32) -> bool {
        // An explicit snapshot row remains authoritative while the asynchronous grid catches up.
        self.core().view().is_some_and(|view| {
            view.runes.iter().any(|rune| {
                rune.pos.x.to_f32() == x
                    && rune.pos.y.to_f32() == y
                    && !self.fog.hides(rune.seen_by)
            })
        }) || self.fog.n > 0 && self.fog.lit(x, y)
    }

    pub fn allows(&self, event: &EventKind, subject: EventSubject<'_>) -> bool {
        self.fact(event, subject)
    }

    pub fn allows_unit(&self, unit: &UnitView) -> bool {
        self.core().view().is_some()
            && (!self.fog.on
                || if bota_core::kind_def(unit.kind).always_seen {
                    // A map object's location is public, but its effects still require sight.
                    self.sees_point(unit.pos)
                } else {
                    !self.fog.hides(unit.seen_by)
                })
    }

    pub fn visible_unit(&self, id: EntityId) -> Option<&UnitView> {
        self.core()
            .view()?
            .unit(id)
            .filter(|unit| self.allows_unit(unit))
    }

    /// Side streams omit visibility bits because the server already selected their audience.
    pub fn knowledge(&self, seen: Option<bota_proto::SeenBy>) -> bota_proto::SeenBy {
        match self.core().view().and_then(|view| view.viewer) {
            Some(team) => bota_proto::SeenBy {
                radiant: team == bota_proto::Team::Radiant,
                dire: team == bota_proto::Team::Dire,
            },
            None => seen.unwrap_or_default(),
        }
    }

    pub fn allows_point(&self, x: f32, y: f32) -> bool {
        x.is_finite() && y.is_finite() && (!self.fog.on || self.sees_point_xy(x, y))
    }

    pub fn allows_projectile(&self, projectile: &bota_proto::ProjectileView) -> bool {
        self.core().view().is_some() && !self.fog.hides(projectile.seen_by)
    }

    fn projectile(&self, id: EntityId) -> Option<bool> {
        let view = self.core().view()?;
        if self.projectiles.stamp != Some((self.epoch.0, view.tick, self.core().fog_team())) {
            return None;
        }
        self.projectiles
            .rows
            .binary_search_by_key(&id, |(id, _)| *id)
            .ok()
            .map(|index| self.projectiles.rows[index].1)
    }

    pub fn sees_projectile(&self, id: EntityId) -> bool {
        self.projectile(id).unwrap_or(false)
    }

    /// Full replays keep both teams' conversations; presentation follows the selected eyes.
    pub fn allows_chat(&self, heard: &bota_proto::Heard) -> bool {
        use bota_proto::{Audience, Speaker};
        let core = self.core();
        let side = |slot| {
            core.statics()
                .and_then(|statics| statics.pick(slot))
                .map(|pick| pick.team)
                .or_else(|| {
                    core.lobby
                        .slots
                        .iter()
                        .find(|seat| seat.slot == slot)
                        .map(|seat| seat.team)
                })
        };
        let viewer = core
            .fog_team()
            .or_else(|| core.pov.or(core.my_slot).or(core.lobby.slot).and_then(side));
        match (&heard.from, heard.line.to) {
            (Speaker::Seat(slot), Audience::Team) => {
                side(*slot).is_some_and(|speaker| viewer.is_none_or(|team| team == speaker))
            }
            (Speaker::Seat(_), Audience::All) => true,
            (Speaker::Spectator(_), Audience::Spectators) => self.core().my_slot.is_none(),
            _ => false,
        }
    }

    pub fn death_witness(&self, event: &EventKind) -> Option<&UnitView> {
        let EventKind::Died { unit, .. } = event else {
            return None;
        };
        let core = self.core();
        let index = core.told.iter().position(|known| known == event)?;
        core.before_event(index, *unit)
    }

    /// The casting side knows its own aim even after a target dies or leaves sight.
    /// Union knowledge includes each side's own orders, just as it includes purchases.
    pub fn aim_knowledge(&self, caster: Option<EntityId>) -> Option<bota_proto::SeenBy> {
        let caster = caster?;
        let team = self.unit(caster).map(|unit| unit.team).or_else(|| {
            self.core()
                .view()?
                .players
                .iter()
                .find(|player| player.unit == Some(caster))
                .map(|player| player.team)
        })?;
        if !matches!(team, bota_proto::Team::Radiant | bota_proto::Team::Dire) {
            return None;
        }
        Some(bota_proto::SeenBy {
            radiant: team == bota_proto::Team::Radiant,
            dire: team == bota_proto::Team::Dire,
        })
    }

    pub fn flights(&self) -> Vec<bota_client_core::Flight> {
        let Some(view) = self.core().view() else {
            return Vec::new();
        };
        bota_client_core::flights(view, self.core().prev_view(), self.core().smooth.alpha())
            .into_iter()
            .filter(|flight| self.allows_projectile(&flight.missile))
            .collect()
    }

    fn placement(&self, subject: EventSubject<'_>, default: bool) -> bool {
        !self.fog.on
            || match subject {
                EventSubject::Event => default,
                EventSubject::Unit(unit) => self.allows_unit(unit),
                EventSubject::Point(at) => self.sees_point(at),
            }
    }

    fn shown(&self, subject: EventSubject<'_>, told: bool, default: bool) -> bool {
        // A side stream also carries globally known map objects. Their locations being known
        // does not authorize a positional effect or sound outside actual sight.
        let map_object = matches!(subject, EventSubject::Unit(unit) if bota_core::kind_def(unit.kind).always_seen);
        told && !map_object || self.placement(subject, default)
    }

    fn allows_item_use(
        &self,
        subject: EventSubject<'_>,
        told: bool,
        at: Vec2,
        target: Option<EntityId>,
        caster: Option<EntityId>,
    ) -> bool {
        let target_seen = target.is_none_or(|unit| self.sees_unit(unit));
        match subject {
            EventSubject::Point(point) => {
                self.aim_knowledge(caster)
                    .is_some_and(|seen| !self.fog.hides(Some(seen)))
                    || told
                    || !self.fog.on
                    || self.sees_point(point) && target_seen
            }
            EventSubject::Unit(_) => self.shown(subject, told, false),
            EventSubject::Event => told || !self.fog.on || self.sees_point(at) && target_seen,
        }
    }

    fn announced_death(&self, view: &WorldView, unit: EntityId) -> bool {
        self.unit(unit).map_or_else(
            // An unknown unit is public only if the roster names a player's body.
            || view.players.iter().any(|player| player.unit == Some(unit)),
            |unit| bota_core::kind_def(unit.kind).death_announced,
        )
    }

    fn fact(&self, event: &EventKind, subject: EventSubject<'_>) -> bool {
        let Some(view) = self.core().view() else {
            return false;
        };
        let told = view.viewer.is_some();
        let shown = |default| self.shown(subject, told, default);
        match *event {
            EventKind::Landed { at, .. }
            | EventKind::TreeFelled { at, .. }
            | EventKind::TreeGrew { at }
            | EventKind::RuneSpawned { at, .. } => shown(self.sees_point(at)),
            EventKind::ItemUsed {
                at: Some(at),
                target,
                caster,
                ..
            } => self.allows_item_use(subject, told, at, target, caster),
            EventKind::RuneDenied { unit, at, team, .. } => {
                (told || self.fog.team == Some(team) || self.sees_point(at) && self.sees_unit(unit))
                    && shown(true)
            }
            EventKind::Damaged { target, .. }
            | EventKind::Missed { target, .. }
            | EventKind::Healed { target, .. } => shown(self.sees_unit(target)),
            EventKind::AbilityCast { caster, .. } => shown(self.sees_unit(caster)),
            EventKind::ItemUsed {
                caster, at: None, ..
            } => shown(caster.is_some_and(|id| self.sees_unit(id))),
            EventKind::LevelUp { unit, .. } | EventKind::RuneActivated { unit, .. } => {
                shown(self.sees_unit(unit))
            }
            EventKind::RuneBottled { unit, .. } => {
                // Without a selected side, even disabling fog does not expose private inventory.
                (told
                    || self
                        .fog
                        .team
                        .is_some_and(|team| self.unit(unit).is_some_and(|unit| unit.team == team)))
                    && shown(self.sees_unit(unit))
            }
            EventKind::ItemBought { slot, .. } => {
                (told
                    || self.fog.team.is_none_or(|team| {
                        view.players
                            .iter()
                            .any(|player| player.slot == slot && player.team == team)
                    }))
                    && shown(true)
            }
            EventKind::LotusGrown { pool, .. } | EventKind::LotusPlucked { pool, .. } => {
                shown(self.sees_unit(pool))
            }
            // These deaths, like captures and Wisdom, are public announcements in the protocol.
            EventKind::Died { unit, .. } => {
                // Death removes the victim's vision. Its exact before-state is the evidence,
                // independent of when the asynchronous post-death ground grid finishes.
                match subject {
                    EventSubject::Event => {
                        told || self.sees_unit(unit) || self.announced_death(view, unit)
                    }
                    EventSubject::Unit(before) => self.allows_unit(before),
                    EventSubject::Point(at) => self
                        .death_witness(event)
                        .is_some_and(|before| before.pos == at && self.allows_unit(before)),
                }
            }
            EventKind::StructureDestroyed { .. }
            | EventKind::Captured { .. }
            | EventKind::ShrineActivated { .. }
            | EventKind::Teleported { .. } => shown(true),
        }
    }
}
