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
            .is_some_and(|unit| !self.fog.hides(unit.seen_by))
    }

    fn sees_point(&self, at: Vec2) -> bool {
        // An explicit snapshot row remains authoritative while the asynchronous grid catches up.
        self.core().view().is_some_and(|view| {
            view.runes
                .iter()
                .any(|rune| rune.pos == at && !self.fog.hides(rune.seen_by))
        }) || self.fog.n > 0 && self.fog.lit(at.x.to_f32(), at.y.to_f32())
    }

    pub fn allows(&self, event: &EventKind, subject: EventSubject<'_>) -> bool {
        self.fact(event, subject)
    }

    pub fn allows_unit(&self, unit: &UnitView) -> bool {
        self.core().view().is_some() && self.placement(EventSubject::Unit(unit), false)
    }

    fn placement(&self, subject: EventSubject<'_>, default: bool) -> bool {
        !self.fog.on
            || match subject {
                EventSubject::Event => default,
                EventSubject::Unit(unit) => !self.fog.hides(unit.seen_by),
                EventSubject::Point(at) => self.sees_point(at),
            }
    }

    fn allows_item_use(
        &self,
        subject: EventSubject<'_>,
        told: bool,
        at: Vec2,
        target: Option<EntityId>,
    ) -> bool {
        let target_seen = target.is_none_or(|unit| self.sees_unit(unit));
        // A caster's flash can remain visible while the aimed unit and its point are private.
        (told || !self.fog.on || !matches!(subject, EventSubject::Point(_)) || target_seen)
            && (told || self.placement(subject, self.sees_point(at) && target_seen))
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
        let shown = |default| told || self.placement(subject, default);
        match *event {
            EventKind::Landed { at, .. }
            | EventKind::TreeFelled { at, .. }
            | EventKind::TreeGrew { at }
            | EventKind::RuneSpawned { at, .. } => shown(self.sees_point(at)),
            EventKind::ItemUsed {
                at: Some(at),
                target,
                ..
            } => self.allows_item_use(subject, told, at, target),
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
                    || !self.fog.on && self.fog.team.is_none()
                    || self.fog.team.is_some_and(|team| {
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
                // An announcement carries no position: a derived cue must still see its spot.
                self.placement(
                    subject,
                    told || self.sees_unit(unit) || self.announced_death(view, unit),
                )
            }
            EventKind::StructureDestroyed { .. }
            | EventKind::Captured { .. }
            | EventKind::ShrineActivated { .. }
            | EventKind::Teleported { .. } => shown(true),
        }
    }
}
