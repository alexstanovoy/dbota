//! Presentation belongs to the result being inspected, independently of a live game behind it.

use super::{
    graphs::Metric,
    summary::{Detail, MatchSummary, Row},
};
use bota_proto::{SlotId, Team};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Mvp,
    Radiant,
    Dire,
    Scoreboard,
    Graphs,
}

impl Page {
    pub fn name(self) -> &'static str {
        match self {
            Self::Mvp => "mvp",
            Self::Radiant => "radiant",
            Self::Dire => "dire",
            Self::Scoreboard => "scoreboard",
            Self::Graphs => "graphs",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Context {
    pub match_id: Option<u64>,
    pub mode: String,
    pub when: String,
    pub kind: Kind,
    pub rating: Option<(i32, i32)>,
    pub reason: String,
    pub owner: Option<SlotId>,
    pub history: bool,
    pub replay: Option<Replay>,
    /// Runtime account mapping received from the authenticated service, never from a file.
    pub live_service: Option<String>,
    pub live_accounts: Vec<(SlotId, u64)>,
}

impl Context {
    pub fn same_header(&self, other: &Self) -> bool {
        self.match_id == other.match_id
            && self.mode == other.mode
            && self.when == other.when
            && self.kind == other.kind
            && self.rating == other.rating
            && self.reason == other.reason
            && self.owner == other.owner
            && self.live_service == other.live_service
            && self.live_accounts == other.live_accounts
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    Ranked,
    Unranked,
    Direct,
    Local,
    #[default]
    Unknown,
}

impl Kind {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Ranked => " · RANKED",
            Self::Unranked => " · UNRANKED",
            Self::Direct => " · DIRECT GAME",
            Self::Local | Self::Unknown => "",
        }
    }
}

#[derive(Clone)]
pub enum Replay {
    File(std::path::PathBuf),
    Blob(Arc<[u8]>),
    Saved(String),
}

impl std::fmt::Debug for Replay {
    fn fmt(&self, format: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) => format.debug_tuple("File").field(path).finish(),
            Self::Saved(name) => format.debug_tuple("Saved").field(name).finish(),
            Self::Blob(bytes) => write!(format, "Replay blob ({} bytes)", bytes.len()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct View {
    /// Imported icon aliases in visual fixtures only. Runtime items always come from meta ids.
    pub illustrated_items: Vec<IllustratedInventory>,
    pub summary: Arc<MatchSummary>,
    pub context: Context,
    pub page: Page,
    pub player: Option<SlotId>,
    pub metric: Metric,
    pub focus: Option<SlotId>,
    pub picker: bool,
    pub notice: Option<String>,
    /// Preview-only emphasis; runtime hover lives on the drawn components.
    pub hover_second: Option<u32>,
    pub hover_player: Option<SlotId>,
}

impl View {
    pub fn profile_target(&self, slot: SlotId) -> Option<crate::main_menu::player_profile::Target> {
        self.row(slot)?;
        if !self.context.history
            && let Some(service) = &self.context.live_service
            && let Some((_, account)) = self
                .context
                .live_accounts
                .iter()
                .find(|(seat, _)| *seat == slot)
        {
            return Some(crate::main_menu::player_profile::Target {
                service: service.clone(),
                account: *account,
            });
        }
        let profiles = self.summary.recording.as_ref()?.profiles.as_ref()?;
        Some(crate::main_menu::player_profile::Target {
            service: bota_radovid::shareable_matchmaker_identity(profiles.matchmaker()?)?,
            account: profiles.account(slot.0)?,
        })
    }

    pub fn new(summary: Arc<MatchSummary>, context: Context) -> Self {
        let mut view = Self {
            illustrated_items: Vec::new(),
            summary,
            context,
            page: Page::Scoreboard,
            player: None,
            metric: Metric::Advantage,
            focus: None,
            picker: false,
            notice: None,
            hover_second: None,
            hover_player: None,
        };
        if view.summary.detail == Detail::Meta && view.mvp().is_some() {
            view.page = Page::Mvp;
        }
        view
    }

    pub fn pages(&self) -> Vec<Page> {
        if self.summary.rows.is_empty() {
            return vec![Page::Scoreboard];
        }
        let mut pages = Vec::with_capacity(5);
        if self.mvp().is_some() {
            pages.push(Page::Mvp);
        }
        pages.extend([Page::Radiant, Page::Dire, Page::Scoreboard]);
        if self
            .summary
            .recording
            .as_ref()
            .is_some_and(|recording| !recording.restricted)
        {
            pages.push(Page::Graphs);
        }
        pages
    }

    pub fn select(&mut self, page: Page) {
        if self.pages().contains(&page) {
            self.page = page;
            self.player = None;
            self.picker = false;
        }
    }

    pub fn advance(&mut self, forward: bool) {
        let pages = self.pages();
        let current = pages
            .iter()
            .position(|page| *page == self.page)
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % pages.len()
        } else {
            (current + pages.len() - 1) % pages.len()
        };
        self.select(pages[next]);
    }

    pub fn mvp(&self) -> Option<&Row> {
        if self.summary.detail == Detail::Meta
            && self.summary.recording.as_ref().is_none_or(|recording| {
                !recording.roster_complete
                    || !recording.complete()
                    || recording
                        .samples
                        .iter()
                        .any(|seat| seat.at(self.summary.seconds).is_none())
            })
        {
            return None;
        }
        let winner = self.summary.winner?;
        if winner == Team::Neutral {
            return self
                .summary
                .rows
                .iter()
                .max_by_key(|row| (impact(row), std::cmp::Reverse(row.slot)));
        }
        self.ranked(winner).into_iter().next()
    }

    pub fn key_players(&self) -> Vec<&Row> {
        if self.summary.rows.len() <= 2 {
            return Vec::new();
        }
        let Some(mvp) = self.mvp() else {
            return Vec::new();
        };
        let mut players = Vec::new();
        if let Some(row) = self.ranked(mvp.team).get(1) {
            players.push(*row);
        }
        let other = if mvp.team == Team::Radiant {
            Team::Dire
        } else {
            Team::Radiant
        };
        if let Some(row) = self.ranked(other).first() {
            players.push(*row);
        }
        players
    }
    pub fn ranked(&self, team: Team) -> Vec<&Row> {
        let mut rows: Vec<_> = self
            .summary
            .rows
            .iter()
            .filter(|row| row.team == team)
            .collect();
        rows.sort_by_key(|row| (std::cmp::Reverse(impact(row)), row.slot));
        rows
    }

    pub fn row(&self, slot: SlotId) -> Option<&Row> {
        self.summary.rows.iter().find(|row| row.slot == slot)
    }

    pub fn open_player(&mut self, slot: SlotId) {
        if self.summary.detail == Detail::Meta && self.row(slot).is_some() {
            self.player = Some(slot);
            self.picker = false;
        }
    }
}

/// The mockup's impact ordering, evaluated as an integer ratio rather than a floating score.
fn impact(row: &Row) -> i64 {
    i64::from(row.kills) * 12_000 + i64::from(row.assists) * 6_000 - i64::from(row.deaths) * 9_000
        + i64::from(row.last_hits.unwrap_or(0)) * 150
        + i64::from(row.hero_damage.unwrap_or(0)) * 4
        + i64::from(row.tower_damage.unwrap_or(0)) * 6
        + i64::from(row.healing.unwrap_or(0)) * 6
        + i64::from(row.net_worth.unwrap_or(0)) * 3
}

#[derive(Clone, Debug)]
pub enum Action {
    Close,
    Dismiss,
    Continue,
    Watch,
    Previous,
    Next,
    Page(Page),
    Player(SlotId),
    Profile(crate::main_menu::player_profile::Target),
    Back,
    Picker,
    Metric(Metric),
    Focus(SlotId),
}

#[derive(bevy::prelude::Component, Clone)]
#[require(
    bevy::ui_widgets::Button,
    bevy::picking::hover::Hovered,
    bevy::picking::hover::DirectlyHovered,
    bevy::input_focus::tab_navigation::TabIndex
)]
pub struct Button(pub Action);

#[derive(Clone, Debug)]
pub struct IllustratedInventory {
    pub slot: SlotId,
    pub items: Vec<Option<String>>,
    pub purchases: Vec<(String, i64)>,
}
