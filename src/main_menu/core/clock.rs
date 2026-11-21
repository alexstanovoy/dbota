use std::time::Duration;

use bevy::platform::time::Instant;

use super::{IDLE_HANG_UP, MenuCore, Page};

impl MenuCore {
    /// When a timer next changes the menu. Incoming control messages and worker results
    /// wake the window independently.
    pub(crate) fn next_wake(&self, now: Instant) -> Option<Instant> {
        let page = self.page_beneath();
        let searches = self.client.as_ref().map(bota_radovid::Client::searches);
        let search_clocks = searches.into_iter().flat_map(|searches| {
            searches.entries().iter().flat_map(move |search| {
                let waited = search.waited_ms();
                [
                    (waited < u32::MAX).then(|| up(now, waited)),
                    search
                        .offer
                        .as_ref()
                        .and_then(|offer| down(now, offer.left_ms())),
                ]
                .into_iter()
                .flatten()
            })
        });
        let lobby_clock = self.lobby.as_ref().and_then(|(lobby, received)| {
            let elapsed = u32::try_from(now.saturating_duration_since(*received).as_millis())
                .unwrap_or(u32::MAX);
            down(now, lobby.starting_in_ms?.saturating_sub(elapsed))
        });
        let opening = self.opening_since.filter(|_| self.launching).map(|start| {
            start + Duration::from_secs(now.saturating_duration_since(start).as_secs() + 1)
        });
        let dates = (matches!(page, Page::Home | Page::Profile)
            && !self.local_history.entries().is_empty())
        .then(|| {
            let wall = crate::replays::wall_clock();
            now + Duration::from_secs(60 - wall.as_secs() % 60)
                - Duration::from_nanos(u64::from(wall.subsec_nanos()))
        });

        [
            self.retry_at.filter(|_| self.account),
            self.leave_at,
            self.leaving.as_ref().map(|leaving| leaving.until),
            self.idle_since.map(|since| since + IDLE_HANG_UP),
            self.queue_full_until.filter(|at| *at > now),
            self.lobby_ui.search_due,
            self.lobby_ui.queued_until,
            self.lobby_ui
                .editor
                .as_ref()
                .and_then(|editor| editor.saving_until),
            self.result_replay_deadline(),
            self.next_replay_fetch(),
            self.local_history.next_save(),
            self.local_meta.next_retry(now),
            (self.page == Page::Leaderboard && self.link_ready())
                .then(|| self.board.next_wake(now))
                .flatten(),
            matches!(self.page, Page::PlayerProfile(_))
                .then(|| self.profiles.next_wake())
                .flatten(),
            searches.and_then(|searches| down(now, searches.cooldown_left_ms())),
            lobby_clock,
            opening,
            dates,
        ]
        .into_iter()
        .flatten()
        .chain(self.lobby_ui.deadlines.iter().map(|(_, until, _)| *until))
        .chain(search_clocks)
        .min()
    }
}

fn up(now: Instant, elapsed_ms: u32) -> Instant {
    now + Duration::from_millis(u64::from(1_000 - elapsed_ms % 1_000))
}

fn down(now: Instant, left_ms: u32) -> Option<Instant> {
    (left_ms > 0).then(|| now + Duration::from_millis(u64::from((left_ms - 1) % 1_000 + 1)))
}

#[cfg(test)]
#[test]
fn visible_counters_wake_at_the_next_displayed_second() {
    let now = Instant::now();
    assert_eq!(up(now, 1_200), now + Duration::from_millis(800));
    assert_eq!(down(now, 1_200), Some(now + Duration::from_millis(200)));
    assert_eq!(down(now, 1_000), Some(now + Duration::from_secs(1)));
    assert_eq!(down(now, 0), None);
}
