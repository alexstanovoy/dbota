//! Dota's player colours: each seat's own (`HeroSlot0Color` to
//! `HeroSlot9Color` of the install's `dotastyles`: Radiant's five seats blue,
//! teal, purple, yellow and orange, Dire's pink, olive, light blue, dark
//! green and brown), read from the cache. A seat whose colour the cache lacks takes its side's
//! colour, which is absolute: Radiant green, Dire red, whoever watches.

use bevy::prelude::*;
use bota_proto::{SlotId, Team, WorldView};

use crate::dota_text::Texts;

/// Seats a side has in Dota, each with a colour of its own.
const SIDE_SEATS: usize = 5;

/// The ten colours of Dota's player slots, as far as the cache has them.
#[derive(Resource, Default)]
pub struct PlayerColours([Option<Color>; 2 * SIDE_SEATS]);

impl PlayerColours {
    pub fn load(texts: &Texts) -> PlayerColours {
        PlayerColours(std::array::from_fn(|at| {
            texts.style(&format!("HeroSlot{at}Color"))
        }))
    }

    /// Dota's colour of a side's `rank`-th seat.
    pub fn at(&self, team: Team, rank: usize) -> Color {
        let first = match team {
            Team::Radiant => 0,
            Team::Dire => SIDE_SEATS,
            Team::Neutral => return crate::overlay::team_color(Team::Neutral),
        };
        (rank < SIDE_SEATS)
            .then(|| self.0[first + rank])
            .flatten()
            .unwrap_or_else(|| crate::overlay::team_color(team))
    }

    /// Dota's colour of a seat: its place among its side's seats picks it.
    pub fn of(&self, view: &WorldView, slot: SlotId) -> Color {
        let Some(player) = view.player(slot) else {
            return Color::WHITE;
        };
        let rank = view
            .players
            .iter()
            .filter(|p| p.team == player.team && p.slot < slot)
            .count();
        self.at(player.team, rank)
    }
}
