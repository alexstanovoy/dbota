//! Peer lobby data is bounded before it reaches forms, projections or chat.
use super::*;
use bota_radovid::proto::{
    MAX_LOBBY_LIST, MAX_LOBBY_NAME_LEN, MAX_NICKNAME_LEN, MAX_RATED_MODES, MAX_RULESET_INFO_LEN,
    MAX_SEATS, MAX_VERSION_LEN, RulesetInfo, valid_ruleset_id,
};

// The protocol caps rated modes, not the complete catalogue. This is the client's displayed
// catalogue budget; larger catalogues are explicitly reported rather than expanded each frame.
pub(super) const CATALOGUE_BUDGET: usize = MAX_RATED_MODES;

fn peer_text(raw: &str, bytes: usize, characters: usize) -> String {
    let mut end = raw.len().min(bytes);
    while !raw.is_char_boundary(end) {
        end -= 1;
    }
    text::line(&raw[..end], characters)
        .chars()
        .take(characters)
        .collect()
}

fn valid_teams(teams: &[u8]) -> bool {
    teams.len() == 2
        && !teams.contains(&0)
        && teams.iter().map(|n| usize::from(*n)).sum::<usize>() <= MAX_SEATS
}

pub(super) fn sanitise_state(mut state: LobbyState) -> Option<LobbyState> {
    if !valid_teams(&state.teams) || !valid_ruleset_id(&state.ruleset.0) {
        return None;
    }
    state.name = peer_text(&state.name, MAX_LOBBY_NAME_LEN * 4, MAX_LOBBY_NAME_LEN);
    state.members.truncate(usize::from(u8::MAX)); // LobbySummary's member count is u8.
    state.members.shrink_to_fit();
    state.retired_pending = Vec::new();
    for member in &mut state.members {
        member.nickname = peer_text(&member.nickname, MAX_NICKNAME_LEN, MAX_NICKNAME_LEN);
        if let Some(version) = &mut member.version {
            *version = crate::player_name::version_text(version, MAX_VERSION_LEN);
        }
    }
    Some(state)
}

impl MenuCore {
    pub(super) fn receive_lobby_list(&mut self, list: Vec<LobbySummary>) {
        self.lobbies = list
            .into_iter()
            .take(MAX_LOBBY_LIST)
            .filter_map(|mut row| {
                if !valid_ruleset_id(&row.ruleset.0) {
                    return None;
                }
                row.name = peer_text(&row.name, MAX_LOBBY_NAME_LEN * 4, MAX_LOBBY_NAME_LEN);
                row.owner_nickname =
                    peer_text(&row.owner_nickname, MAX_NICKNAME_LEN, MAX_NICKNAME_LEN);
                row.version = row
                    .version
                    .map(|version| crate::player_name::version_text(&version, MAX_VERSION_LEN));
                row.seats = row.seats.min(MAX_SEATS as u8);
                Some(row)
            })
            .collect();
        self.lobbies.shrink_to_fit();
        self.lobby_ui.controls_dirty = true;
    }

    pub(super) fn receive_lobby_catalogue(&mut self, list: &[RulesetInfo]) {
        let kind = self
            .client
            .as_ref()
            .and_then(|client| client.me())
            .map(|me| me.kind);
        let mut catalogue: Vec<_> = list
            .iter()
            .filter(|rule| {
                rule.game == bota_radovid::GAME
                    && rule.game_fingerprint == bota_radovid::fingerprint()
                    && kind.is_none_or(|kind| rule.audience.admits(kind))
                    && valid_ruleset_id(&rule.id.0)
                    && valid_teams(&rule.teams)
                    && rule.info.len() <= MAX_RULESET_INFO_LEN
                    && usize::from(rule.lobby_min_players) <= MAX_SEATS
            })
            .take(CATALOGUE_BUDGET + 1)
            .map(|rule| {
                let mut safe = rule.clone();
                safe.name = peer_text(&safe.name, text::NAME * 4, text::NAME);
                safe
            })
            .collect();
        let limited = catalogue.len() > CATALOGUE_BUDGET;
        catalogue.truncate(CATALOGUE_BUDGET);
        self.catalogue = catalogue;
        self.lobby_ui.controls_dirty = true;
        self.lobby_ui.state_dirty = true;
        let notice = format!("Only the first {CATALOGUE_BUDGET} game modes are shown.");
        if limited {
            self.tell(notice, false);
        } else if self
            .status
            .as_ref()
            .is_some_and(|status| status.text == notice)
        {
            self.status = None;
        }
    }

    pub(in crate::main_menu) fn menu_rulesets(&self) -> impl Iterator<Item = &RulesetInfo> {
        let connected = self.client.is_some();
        let kind = self
            .client
            .as_ref()
            .and_then(|c| c.me())
            .map_or(AccountKind::Human, |me| me.kind);
        self.catalogue
            .iter()
            .filter(move |r| connected && r.audience.admits(kind))
    }

    pub(super) fn lobby_catalogue(&self) -> impl Iterator<Item = &RulesetInfo> {
        self.menu_rulesets().filter(|r| !bota_radovid::is_demo(r))
    }

    pub(super) fn default_lobby_mode(&self) -> Option<RulesetId> {
        self.lobby_catalogue()
            .find(|r| r.lobby_default)
            .or_else(|| self.lobby_catalogue().next())
            .map(|r| r.id.clone())
    }
}
