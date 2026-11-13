//! Only an authenticated delivery may attach the configured service's public account mapping.
use bota_radovid::proto::MatchResult;
use bota_replay::meta::{Profiles, decode, with_profiles};

pub(super) fn from_result(service: &str, result: &MatchResult) -> Option<Profiles> {
    if result.seats.len() > bota_replay::meta::MAX_PROFILE_SEATS {
        return None;
    }
    let Some(service) = bota_radovid::shareable_matchmaker_identity(service) else {
        return Some(Profiles::default());
    };
    Profiles::new(
        Some(service),
        result
            .seats
            .iter()
            .map(|seat| (seat.slot, Some(seat.account.0)))
            .collect(),
    )
}

pub(super) fn same_service(source: &str, configured: &str) -> bool {
    bota_radovid::same_matchmaker(source, configured)
}

pub(super) fn stamp_meta(bytes: &[u8], match_id: u64, profiles: &Profiles) -> Option<Vec<u8>> {
    if decode(bytes).ok()?.header.match_id != match_id {
        return None;
    }
    with_profiles(bytes, profiles)
}

pub(super) fn stamp_replay(bytes: &mut Vec<u8>, match_id: u64, profiles: &Profiles) {
    let Some(meta) = bota_client_core::meta_in_replay(&mut std::io::Cursor::new(&*bytes))
        .ok()
        .flatten()
    else {
        return;
    };
    let Some(enriched) = stamp_meta(&meta, match_id, profiles) else {
        return;
    };
    let Some(frame) = bota_replay::trailer::frame(&enriched) else {
        return;
    };
    let Some(prefix) = bytes
        .len()
        .checked_sub(meta.len() + bota_replay::trailer::OVERHEAD)
    else {
        return;
    };
    if prefix.saturating_add(frame.len()) as u64 > crate::replays::MAX_BYTES {
        return;
    }
    bytes.truncate(prefix);
    bytes.extend_from_slice(&frame);
}
