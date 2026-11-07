//! Acknowledged decisions belong to the menu session, beyond one control connection.
use bota_radovid::{
    Client,
    proto::{Activities, Event, ProposalId, QueueEntryId},
};
const MAX_ACKNOWLEDGED: usize = 16;

#[derive(Default)]
pub(super) struct Accepted(Vec<(QueueEntryId, ProposalId)>);

impl Accepted {
    pub fn contains(&self, entry: QueueEntryId, proposal: ProposalId) -> bool {
        self.0.contains(&(entry, proposal))
    }

    pub fn remember(&mut self, client: &Client, proposal: ProposalId) {
        let Some(search) = client.searches().entries().iter().find(|search| {
            search
                .offer
                .as_ref()
                .is_some_and(|offer| offer.proposal == proposal)
        }) else {
            return;
        };
        self.insert(search.entry, proposal);
    }

    fn insert(&mut self, entry: QueueEntryId, proposal: ProposalId) {
        self.0.retain(|(_, old)| *old != proposal);
        if self.0.len() == MAX_ACKNOWLEDGED {
            self.0.remove(0);
        }
        self.0.push((entry, proposal));
    }

    pub fn resumed(&mut self, snapshot: &Activities) {
        self.0.retain(|(entry, id)| {
            snapshot
                .proposals
                .iter()
                .any(|offer| offer.entry == *entry && offer.proposal == *id)
        });
    }

    pub fn update(&mut self, client: &Client) {
        self.0.retain(|(entry, id)| {
            client.searches().entries().iter().any(|search| {
                search.entry == *entry
                    && search
                        .offer
                        .as_ref()
                        .is_some_and(|offer| offer.proposal == *id && offer.left_ms() > 0)
            })
        });
    }

    pub fn event(&mut self, event: &Event) {
        match event {
            Event::ProposalFailed { proposal, .. } => self.remove(*proposal),
            Event::QueueLeft { entry, .. } => self.0.retain(|(known, _)| known != entry),
            _ => {}
        }
    }

    pub fn remove(&mut self, proposal: ProposalId) {
        self.0.retain(|(_, id)| *id != proposal);
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_the_last_sixteen_acknowledgements_survive_and_terminal_events_remove_them() {
        let mut accepted = Accepted::default();
        for id in 1..=20 {
            accepted.insert(QueueEntryId(id), ProposalId(id));
        }
        assert!(!accepted.contains(QueueEntryId(4), ProposalId(4)));
        assert!(accepted.contains(QueueEntryId(5), ProposalId(5)));
        accepted.event(&Event::ProposalFailed {
            proposal: ProposalId(5),
            back_in_queue: true,
        });
        assert!(!accepted.contains(QueueEntryId(5), ProposalId(5)));
        accepted.event(&Event::QueueLeft {
            entry: QueueEntryId(6),
            reason: bota_radovid::proto::QueueLeftReason::Cancelled,
            cooldown_ms: 0,
        });
        assert!(!accepted.contains(QueueEntryId(6), ProposalId(6)));
        assert!(accepted.contains(QueueEntryId(20), ProposalId(20)));
    }
}
