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
            .map(|seat| (seat.slot, (seat.account.0!=0).then_some(seat.account.0)))
            .collect(),
    )
}

#[derive(PartialEq,Eq)]
enum DeliveryIdentity {
    Local,
    Service(String),
}
impl DeliveryIdentity {
    fn of(service:&str)->Option<Self> {
        if service.is_empty() { Some(Self::Local) }
        else { bota_radovid::matchmaker_identity(service).map(Self::Service) }
    }
}

pub(super) fn same_service(source: &str, configured: &str) -> bool {
    DeliveryIdentity::of(source).zip(DeliveryIdentity::of(configured)).is_some_and(|(source,configured)|source==configured)
}

pub(super) fn stamp_meta(bytes: &[u8], match_id: u64, profiles: &Profiles) -> Option<Vec<u8>> {
    let metadata=decode(bytes).ok()?;
    if metadata.header.match_id != match_id { return None; }
    let snapshot=metadata.profiles()?;
    if !bota_radovid::same_matchmaker(snapshot.matchmaker()?,profiles.matchmaker()?) {
        return None;
    }
    // A participant's private result must not restore a binding withheld at match time.
    let seats=snapshot.seats().iter().map(|(slot,account)| {
        (*slot,account.filter(|account|profiles.account(*slot)==Some(*account)))
    }).collect();
    let constrained=Profiles::new(snapshot.matchmaker().map(str::to_owned),seats)?;
    with_profiles(bytes, &constrained)
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

#[cfg(test)]
mod delivery_tests {
    use super::*;
    #[test]
    fn local_delivery_has_an_identity_but_an_invalid_remote_address_does_not() {
        assert!(same_service("",""),"the local source is one canonical identity");
        assert!(!same_service("not a URL","not a URL"),"arbitrary equal text is not an identity");
        assert!(!same_service("","wss://match.example"));
        assert!(same_service("WSS://MATCH.example:443/","wss://match.example/control"));
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    #[test]
    fn a_private_result_cannot_restore_a_hidden_snapshot_binding() {
        let source="wss://match.example/control";
        let mut bytes=crate::postgame::preview::metadata("match_details");
        let id=decode(&bytes).expect("metadata").header.match_id;
        bytes.extend(bota_proto::encode_frame_to_vec(&(13_u8,Some(source),vec![(0_u8,None::<u64>)])).expect("server visibility snapshot"));
        let proposal=Profiles::new(Some(source.into()),vec![(0,Some(42))]).expect("private participant");
        let kept=stamp_meta(&bytes,id,&proposal).expect("retain server snapshot");
        assert_eq!(decode(&kept).expect("read").profiles().expect("context").account(0),None);
    }
    #[test]
    fn a_private_result_cannot_invent_bindings_without_a_match_time_snapshot() {
        let bytes=crate::postgame::preview::metadata("match_details");
        let id=decode(&bytes).expect("metadata").header.match_id;
        let proposal=Profiles::new(Some("wss://match.example/control".into()),vec![(0,Some(42))]).expect("private participant");
        assert!(stamp_meta(&bytes,id,&proposal).is_none());
    }
}
