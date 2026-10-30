//! The `fx` lines of the manifest with their subjects resolved to our ids, once, so a frame asks a
//! line about a fact by comparing numbers.

use bevy::prelude::*;
use bota_proto::{
    AbilityId, EffectId, HeroId, Inflictor, ItemId, RuneKind, Team, UnitKind, UnitView,
};

use super::line::{Fx, Trigger};
use crate::manifest::Manifest;

/// Which units a `unit.<npc>` subject names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Units {
    /// A hero: `npc_dota_hero_<name>`.
    Hero(HeroId),
    /// A lane creep of a side: `npc_dota_creep_goodguys_melee`.
    Lane(UnitKind, Team),
    /// A ward, a map objective or a neutral creep, by the name its `unit` line of the manifest
    /// gives.
    Npc(String),
}

impl Units {
    /// Whether `unit` is one of them.
    pub fn is(&self, unit: &UnitView) -> bool {
        match self {
            Units::Hero(hero) => unit.hero == Some(*hero),
            Units::Lane(kind, team) => unit.kind == *kind && unit.team == *team,
            Units::Npc(name) => crate::units::npc_unit(unit) == Some(name.as_str()),
        }
    }

    fn of(npc: &str, manifest: &Manifest) -> Option<Units> {
        if let Some(hero) = npc.strip_prefix("npc_dota_hero_") {
            let id = manifest
                .ids
                .iter()
                .find(|(kind, _, name)| kind == "hero" && name == hero)?
                .1;
            return Some(Units::Hero(HeroId(id)));
        }
        let lane = match npc {
            "npc_dota_creep_goodguys_melee" => Some((UnitKind::CreepMelee, Team::Radiant)),
            "npc_dota_creep_goodguys_ranged" => Some((UnitKind::CreepRanged, Team::Radiant)),
            "npc_dota_goodguys_siege" => Some((UnitKind::CreepSiege, Team::Radiant)),
            "npc_dota_creep_badguys_melee" => Some((UnitKind::CreepMelee, Team::Dire)),
            "npc_dota_creep_badguys_ranged" => Some((UnitKind::CreepRanged, Team::Dire)),
            "npc_dota_badguys_siege" => Some((UnitKind::CreepSiege, Team::Dire)),
            _ => None,
        };
        if let Some((kind, team)) = lane {
            return Some(Units::Lane(kind, team));
        }
        manifest.unit_art(npc).map(|_| Units::Npc(npc.to_owned()))
    }
}

/// What a line is about, as our ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    /// Items of a Dota key (a key ending in `*` names every item that starts with it).
    Items(Vec<ItemId>),
    /// Abilities of a Dota key, on the same terms.
    Abilities(Vec<AbilityId>),
    /// The effects that are the Dota modifier of a name.
    Effects(Vec<EffectId>),
    Units(Units),
    /// A kind of rune, by the name of its art (`rune.haste`).
    Runes(RuneKind),
}

/// A line and its subject.
#[derive(Clone, Debug)]
pub struct Rule {
    pub fx: Fx,
    pub subject: Subject,
}

/// Every line of the manifest that resolved, in the manifest's order.
#[derive(Resource, Default)]
pub struct Rules(pub Vec<Rule>);

impl Rules {
    /// The rules of a trigger.
    pub fn of(&self, trigger: Trigger) -> impl Iterator<Item = (usize, &Rule)> {
        self.0
            .iter()
            .enumerate()
            .filter(move |(_, rule)| rule.fx.trigger == trigger)
    }
}

impl Rule {
    /// Resolves a line against the manifest's ids.
    pub fn resolve(fx: &Fx, manifest: &Manifest) -> Result<Rule, String> {
        let (kind, name) = fx.subject_parts();
        let ids = |kind: &str| -> Vec<u16> {
            let matches = |candidate: &str| match name.strip_suffix('*') {
                Some(prefix) => candidate.starts_with(prefix),
                None => candidate == name,
            };
            manifest
                .ids
                .iter()
                .filter(|(k, _, n)| k == kind && matches(n))
                .map(|(_, id, _)| *id)
                .collect()
        };
        let subject = match kind {
            "item" => Subject::Items(ids("item").into_iter().map(ItemId).collect()),
            "ability" => Subject::Abilities(ids("ability").into_iter().map(AbilityId).collect()),
            "modifier" => Subject::Effects(ids("modifier").into_iter().map(EffectId).collect()),
            "unit" => Subject::Units(Units::of(name, manifest).ok_or("no such unit")?),
            "hero" => {
                let npc = format!("npc_dota_hero_{name}");
                Subject::Units(Units::of(&npc, manifest).ok_or("no such hero")?)
            }
            "rune" => Subject::Runes(crate::runes::of_art(name).ok_or("no such rune")?),
            other => return Err(format!("no subject kind '{other}'")),
        };
        let empty = match &subject {
            Subject::Items(ids) => ids.is_empty(),
            Subject::Abilities(ids) => ids.is_empty(),
            Subject::Effects(ids) => ids.is_empty(),
            Subject::Units(_) | Subject::Runes(_) => false,
        };
        if empty {
            return Err(format!("{} names nothing in ids.txt", fx.subject));
        }
        subject_fits(fx, &subject)?;
        Ok(Rule {
            fx: fx.clone(),
            subject,
        })
    }

    /// Whether what put something on a unit, or dealt a blow, is what the line is about.
    pub fn names(&self, by: Option<Inflictor>) -> bool {
        match (&self.subject, by) {
            (Subject::Items(ids), Some(Inflictor::Item(id))) => ids.contains(&id),
            (Subject::Abilities(ids), Some(Inflictor::Ability(id))) => ids.contains(&id),
            _ => false,
        }
    }
}

/// Whether the trigger has a fact about this kind of subject.
fn subject_fits(fx: &Fx, subject: &Subject) -> Result<(), String> {
    let items_or_abilities = matches!(subject, Subject::Items(_) | Subject::Abilities(_));
    let fits = match fx.trigger {
        Trigger::Use | Trigger::Held | Trigger::Store => matches!(subject, Subject::Items(_)),
        Trigger::Cast | Trigger::Casting => matches!(subject, Subject::Abilities(_)),
        Trigger::Land | Trigger::Flies => items_or_abilities,
        Trigger::Hit => !matches!(subject, Subject::Effects(_) | Subject::Runes(_)),
        Trigger::Died => matches!(subject, Subject::Units(_)),
        Trigger::Spawn | Trigger::Activate => matches!(subject, Subject::Runes(_)),
        Trigger::Worn => true,
    };
    if fits {
        Ok(())
    } else {
        Err(format!("{:?} has no fact about {}", fx.trigger, fx.subject))
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Rules>().add_systems(Startup, resolve);
}

/// Resolves the manifest's lines; one that does not resolve is said and left out.
fn resolve(manifest: Res<Manifest>, mut rules: ResMut<Rules>) {
    for fx in &manifest.fx {
        match Rule::resolve(fx, &manifest) {
            Ok(rule) => rules.0.push(rule),
            Err(why) => error!("fx line for {}: {why}", fx.subject),
        }
    }
}
