//! LEAVE's word to a game whose seat this client left: the match is left for good. A thread of
//! its own takes the seat once more out of sight, waits until the server admits it (`Welcome`),
//! says `LeaveGame`, and makes sure it arrived: the server answers a `Probe` only once it has read
//! everything before it. A seat the server still holds for the old connection (`SeatHeld`), a
//! connection that breaks before the welcome or around the surrender, are tried again; a refusal
//! for good ends it. All of it within a deadline, and nothing of it reaches a game session: a
//! failure here never ends the client.

use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::time::{Duration, Instant};

use bota_client_core::{DEFAULT_IO_TIMEOUT, Link, Redial, Transport};
use bota_proto::{ClientMsg, RejectReason, ServerMsg};

/// How telling the game ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The server has the surrender, or it closed the seat after it (the match was cancelled or
    /// is over: nothing is left to leave).
    Left,
    /// The server says the seat is not this client's (any more): nothing to leave.
    Refused(String),
    /// The game could not be reached before the deadline: it is forgotten untold.
    Unreachable(String),
}

/// What [`Outcome::Left`] is made sure of with: the id of the probe sent after `LeaveGame`.
const PROBE: u32 = 0x1ea7e;

/// The longest wait between two tries.
const LONGEST_WAIT: Duration = Duration::from_secs(1);

/// Opens a connection to the game, its reads and writes bounded by the duration given.
pub type Connect = Box<dyn FnMut(Duration) -> std::io::Result<Link> + Send>;

/// One leave under way, known by its id: an outcome is only ever taken for the leave it belongs
/// to.
pub struct LeaveOp {
    pub id: u64,
    done: Receiver<Outcome>,
}

impl LeaveOp {
    /// Starts telling the game reached by `connect` and greeted with `hello` that the match is
    /// left, until `until`.
    pub fn start(id: u64, connect: Connect, hello: ClientMsg, until: Instant) -> LeaveOp {
        let (tell_done, done) = sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("dbota-leave".to_owned())
            .spawn(move || {
                let _ = tell_done.send(tell(connect, &hello, until));
            });
        if let Err(error) = spawned {
            let (tell_done, done) = sync_channel(1);
            let _ = tell_done.send(Outcome::Unreachable(error.to_string()));
            return LeaveOp { id, done };
        }
        LeaveOp { id, done }
    }

    /// The outcome, once there is one.
    pub fn poll(&mut self) -> Option<Outcome> {
        match self.done.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Outcome::Unreachable("the leave died".to_owned()))
            }
        }
    }
}

/// What one try came to.
enum Try {
    Done(Outcome),
    /// Worth another try, for this reason.
    Again(String),
}

/// Tells the game the match is left, trying again until `until`.
pub fn tell(mut connect: Connect, hello: &ClientMsg, until: Instant) -> Outcome {
    let mut wait = Duration::from_millis(100);
    loop {
        let why = match one_try(&mut connect, hello, until) {
            Try::Done(outcome) => return outcome,
            Try::Again(why) => why,
        };
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Outcome::Unreachable(why);
        }
        std::thread::sleep(wait.min(left));
        wait = (wait * 2).min(LONGEST_WAIT);
    }
}

/// The time left before `until`, at most what one read or write may take.
fn io_time(until: Instant) -> Option<Duration> {
    let left = until.saturating_duration_since(Instant::now());
    (!left.is_zero()).then(|| left.min(DEFAULT_IO_TIMEOUT))
}

fn one_try(connect: &mut Connect, hello: &ClientMsg, until: Instant) -> Try {
    let Some(io) = io_time(until) else {
        return Try::Again("out of time".to_owned());
    };
    let mut link = match connect(io) {
        Ok(link) => link.with_redial(Redial::OFF),
        Err(error) => return Try::Again(format!("no connection: {error}")),
    };
    if let Err(error) = link.send(hello) {
        return Try::Again(format!("the greeting was not sent: {error}"));
    }
    // The seat first: nothing is said before the server admits it.
    loop {
        if io_time(until).is_none() {
            return Try::Again("no welcome in time".to_owned());
        }
        match link.recv() {
            Ok(Some(ServerMsg::Welcome { slot: Some(_), .. })) => break,
            Ok(Some(ServerMsg::Welcome { slot: None, .. })) => {
                return Try::Done(Outcome::Refused("admitted to no seat".to_owned()));
            }
            Ok(Some(ServerMsg::Refused {
                reason: RejectReason::SeatHeld,
            })) => return Try::Again("the seat is still held".to_owned()),
            Ok(Some(ServerMsg::Refused { reason })) => {
                return Try::Done(Outcome::Refused(format!("{reason:?}")));
            }
            Ok(Some(ServerMsg::MatchOver { .. })) => return Try::Done(Outcome::Left),
            Ok(Some(_)) => {}
            Ok(None) => return Try::Again("closed before the welcome".to_owned()),
            Err(error) => return Try::Again(format!("before the welcome: {error}")),
        }
    }
    // The surrender, then a probe behind it: its echo says the server has read both.
    let said = link
        .send(&ClientMsg::LeaveGame)
        .and_then(|()| link.send(&ClientMsg::Probe { id: PROBE }));
    if let Err(error) = said {
        return Try::Again(format!("the surrender was not sent: {error}"));
    }
    loop {
        if io_time(until).is_none() {
            return Try::Again("no word back in time".to_owned());
        }
        match link.recv() {
            Ok(Some(ServerMsg::Echo { id: PROBE })) => return Try::Done(Outcome::Left),
            Ok(Some(_)) => {}
            // Closed by the server, after the surrender: it took it, or ended the match.
            Ok(None) => return Try::Done(Outcome::Left),
            Err(error) => return Try::Again(format!("around the surrender: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    use bota_client_core::Endpoint;
    use bota_proto::{PlayerId, Role, SlotId, TickMode, decode_payload, encode_frame_to_vec};

    use super::*;

    /// One connection of the fake game server, as the test drives it.
    struct Peer(TcpStream);

    impl Peer {
        /// The next message, or none once the client closed.
        fn recv(&mut self) -> Option<ClientMsg> {
            let mut len = [0u8; 4];
            self.0.read_exact(&mut len).ok()?;
            let mut payload = vec![0u8; u32::from_le_bytes(len) as usize];
            self.0.read_exact(&mut payload).ok()?;
            Some(decode_payload(&payload).expect("a message"))
        }

        fn send(&mut self, msg: &ServerMsg) {
            let frame = encode_frame_to_vec(msg).expect("encoded");
            self.0.write_all(&frame).expect("sent");
        }

        /// The client says nothing for `quiet`.
        fn says_nothing_for(&mut self, quiet: Duration) -> bool {
            self.0.set_read_timeout(Some(quiet)).expect("a timeout");
            let mut byte = [0u8; 1];
            let silent = matches!(
                self.0.read(&mut byte),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
            );
            self.0.set_read_timeout(None).expect("no timeout");
            silent
        }
    }

    fn welcome() -> ServerMsg {
        ServerMsg::Welcome {
            player_id: PlayerId(1),
            slot: Some(SlotId(1)),
            tick_rate: 30,
            mode: TickMode::Lockstep,
        }
    }

    fn hello() -> ClientMsg {
        ClientMsg::hello(
            Role::Player,
            "leaver",
            Some(bota_proto::SeatKey([7; 32])),
            bota_core::WIRE_FINGERPRINT,
        )
    }

    /// What each connection the fake server took was told, in order.
    type Heard = Arc<Mutex<Vec<Vec<String>>>>;

    /// A fake game server answering its connections in turn with `answers`, each given the
    /// connection; what a connection says after its answer, if the answer hands it back, is
    /// noted. An answer that keeps it closes it at once, whatever is unread.
    fn serve(answers: Vec<fn(Peer) -> Option<Peer>>) -> (String, Heard) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("bound").to_string();
        let heard: Heard = Arc::default();
        let noted = Arc::clone(&heard);
        std::thread::spawn(move || {
            for answer in answers {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut said = Vec::new();
                if let Some(mut peer) = answer(Peer(stream)) {
                    peer.0
                        .set_read_timeout(Some(Duration::from_millis(300)))
                        .ok();
                    while let Some(msg) = peer.recv() {
                        said.push(format!("{msg:?}"));
                    }
                }
                noted.lock().expect("noted").push(said);
            }
        });
        (address, heard)
    }

    /// Tells the fake server at `address` within `secs`.
    fn leave(address: &str, secs: u64) -> Outcome {
        let address = address.to_owned();
        let connect: Connect = Box::new(move |io| Link::open(Endpoint::parse(&address)?, io));
        tell(
            connect,
            &hello(),
            Instant::now() + Duration::from_secs(secs),
        )
    }

    /// The welcome, the surrender, then its probe, answered.
    fn admit_and_echo(mut peer: Peer) -> Option<Peer> {
        assert!(matches!(peer.recv(), Some(ClientMsg::Hello { .. })));
        admit_rest(&mut peer);
        Some(peer)
    }

    #[test]
    fn a_seat_still_held_is_asked_for_again_until_it_is_given_back() {
        let (address, heard) = serve(vec![
            |mut peer| {
                assert!(matches!(peer.recv(), Some(ClientMsg::Hello { .. })));
                peer.send(&ServerMsg::Refused {
                    reason: RejectReason::SeatHeld,
                });
                Some(peer)
            },
            admit_and_echo,
        ]);
        assert_eq!(leave(&address, 20), Outcome::Left);
        let heard = heard.lock().expect("heard");
        assert_eq!(heard.len(), 2, "a second connection after the refusal");
    }

    #[test]
    fn the_surrender_waits_for_the_welcome() {
        let (address, _) = serve(vec![|mut peer| {
            assert!(matches!(peer.recv(), Some(ClientMsg::Hello { .. })));
            assert!(
                peer.says_nothing_for(Duration::from_millis(700)),
                "nothing is said before the seat is admitted"
            );
            admit_rest(&mut peer);
            Some(peer)
        }]);
        assert_eq!(leave(&address, 20), Outcome::Left);
    }

    /// [`admit_and_echo`] past the `Hello` already read.
    fn admit_rest(peer: &mut Peer) {
        peer.send(&welcome());
        assert_eq!(peer.recv(), Some(ClientMsg::LeaveGame));
        assert_eq!(peer.recv(), Some(ClientMsg::Probe { id: PROBE }));
        peer.send(&ServerMsg::Echo { id: PROBE });
    }

    #[test]
    fn a_reset_around_the_surrender_says_it_again_on_a_fresh_connection() {
        let (address, heard) = serve(vec![
            |mut peer| {
                assert!(matches!(peer.recv(), Some(ClientMsg::Hello { .. })));
                peer.send(&welcome());
                assert_eq!(peer.recv(), Some(ClientMsg::LeaveGame));
                // The probe arrives and is never read: closing with it unread resets the
                // connection, and no echo went back.
                std::thread::sleep(Duration::from_millis(200));
                None
            },
            admit_and_echo,
        ]);
        assert_eq!(leave(&address, 20), Outcome::Left);
        let deadline = Instant::now() + Duration::from_secs(5);
        while heard.lock().expect("heard").len() < 2 {
            assert!(
                Instant::now() < deadline,
                "the surrender was not said again"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_seat_refused_for_good_is_not_tried_again() {
        let (address, heard) = serve(vec![|mut peer| {
            assert!(matches!(peer.recv(), Some(ClientMsg::Hello { .. })));
            peer.send(&ServerMsg::Refused {
                reason: RejectReason::NotAdmitted,
            });
            Some(peer)
        }]);
        assert_eq!(
            leave(&address, 20),
            Outcome::Refused("NotAdmitted".to_owned())
        );
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(heard.lock().expect("heard").len(), 1, "one connection only");
    }

    #[test]
    fn a_game_that_never_answers_is_given_up_at_the_deadline() {
        // Takes the connection and says nothing, for longer than the leave may take.
        let (address, _) = serve(vec![|mut peer| {
            let _ = peer.says_nothing_for(Duration::from_secs(5));
            Some(peer)
        }]);
        let began = Instant::now();
        let outcome = leave(&address, 2);
        assert!(matches!(outcome, Outcome::Unreachable(_)), "{outcome:?}");
        assert!(
            began.elapsed() < Duration::from_secs(4),
            "bounded by the deadline: {:?}",
            began.elapsed()
        );
    }
}
