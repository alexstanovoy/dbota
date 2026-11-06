//! LEAVE's word to a game whose seat this client left: the match is left for good. A thread of
//! its own takes the seat once more out of sight, waits until the server admits it (`Welcome`),
//! says `LeaveGame`, and makes sure it arrived: the server answers a `Probe` only once it has read
//! everything before it (bota_server answers it on the connection's own thread, ahead of anything
//! queued), so only its echo means the game was told. A seat the server still holds for the old
//! connection (`SeatHeld`), a connection that breaks or closes before the welcome or before the
//! echo, are tried again on a fresh connection; a refusal for good ends it. All of it within a
//! deadline, each connection and read bounded by what is left of it, and nothing of it reaches a
//! game session: a failure here never ends the client. A leave the menu gave up on runs on until
//! its deadline, unheard.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, sync_channel};
use std::time::{Duration, Instant};

use bota_client_core::{DEFAULT_IO_TIMEOUT, Link, Redial, Transport};
use bota_proto::{ClientMsg, RejectReason, ServerMsg};

/// How telling the game ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The server has the surrender: it echoed the probe sent behind it, or it closed the
    /// connection after it and was gone by the next try.
    Left,
    /// The server says the seat is not this client's (any more): nothing to leave.
    Refused(String),
    /// The game could not be reached before the deadline: it is forgotten untold.
    Unreachable(String),
    /// The leave was stopped ([`LeaveOp::cancel`]) before it said anything more.
    Cancelled,
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
    cancelled: Arc<AtomicBool>,
}

impl LeaveOp {
    /// Starts telling the game reached by `connect` and greeted with `hello` that the match is
    /// left, until `until`.
    pub fn start(id: u64, connect: Connect, hello: ClientMsg, until: Instant) -> LeaveOp {
        let (tell_done, done) = sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancelled);
        let spawned = std::thread::Builder::new()
            .name("dbota-leave".to_owned())
            .spawn(move || {
                let _ = tell_done.send(tell(connect, &hello, until, &stop));
            });
        if let Err(error) = spawned {
            let (tell_done, done) = sync_channel(1);
            let _ = tell_done.send(Outcome::Unreachable(error.to_string()));
            return LeaveOp {
                id,
                done,
                cancelled,
            };
        }
        LeaveOp {
            id,
            done,
            cancelled,
        }
    }

    /// Stops the leave: it opens no other connection and says no `LeaveGame` from now on. Its
    /// outcome still comes, once it has stopped.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
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
    /// The server closed the connection after the surrender, before its echo: tried again, and
    /// a server that is gone by then took it, or ended the match with it (bota_server cancels a
    /// match left in the picks and exits, closing its connections before every probe is read).
    ClosedAfterSurrender,
    /// No connection was made: a server that closed after the surrender and is gone now.
    NoConnection(String),
}

/// Tells the game the match is left, trying again until `until`, unless `cancelled` stops it.
pub fn tell(
    mut connect: Connect,
    hello: &ClientMsg,
    until: Instant,
    cancelled: &AtomicBool,
) -> Outcome {
    let mut wait = Duration::from_millis(100);
    let mut closed_after_surrender = false;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Outcome::Cancelled;
        }
        let why = match one_try(&mut connect, hello, until, cancelled) {
            Try::Done(outcome) => return outcome,
            Try::Again(why) => why,
            Try::ClosedAfterSurrender => {
                closed_after_surrender = true;
                "closed before the echo".to_owned()
            }
            // The server that closed after the surrender is gone: nothing is left to leave.
            Try::NoConnection(_) if closed_after_surrender => return Outcome::Left,
            Try::NoConnection(why) => why,
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

fn one_try(
    connect: &mut Connect,
    hello: &ClientMsg,
    until: Instant,
    cancelled: &AtomicBool,
) -> Try {
    let Some(io) = io_time(until) else {
        return Try::Again("out of time".to_owned());
    };
    let mut link = match connect(io) {
        Ok(link) => link.with_redial(Redial::OFF),
        Err(error) => return Try::NoConnection(format!("no connection: {error}")),
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
            // Another wire never admits this client: no try will.
            Ok(Some(ServerMsg::WireMismatch { .. })) => {
                return Try::Done(Outcome::Refused("another wire".to_owned()));
            }
            // A server says no more than Welcome or Refused first: anything else is tried again.
            Ok(Some(ServerMsg::MatchOver { .. })) => {
                return Try::Again("a match over before the welcome".to_owned());
            }
            Ok(Some(_)) => {}
            Ok(None) => return Try::Again("closed before the welcome".to_owned()),
            Err(error) => return Try::Again(format!("before the welcome: {error}")),
        }
    }
    if cancelled.load(Ordering::SeqCst) {
        return Try::Done(Outcome::Cancelled);
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
            // Closed before the echo: whether the surrender was read is not known.
            Ok(None) => return Try::ClosedAfterSurrender,
            Err(error) => return Try::Again(format!("around the surrender: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    use bota_client_core::Endpoint;
    use bota_proto::{PlayerId, Role, SlotId, TickMode, decode_payload, encode_frame_to_vec};

    use super::*;

    /// How long a test waits for what must come, at most: a hang guard, not a measure.
    const HANG_GUARD: Duration = Duration::from_secs(60);

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

        /// Waits until the client's next bytes have arrived, leaving them unread.
        fn until_more_arrived(&mut self) {
            self.0
                .set_read_timeout(Some(HANG_GUARD))
                .expect("a timeout");
            let mut byte = [0u8; 1];
            assert!(
                matches!(self.0.peek(&mut byte), Ok(1..)),
                "the client said nothing more"
            );
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

    /// The fake server's thread, and the count of the connections it took.
    struct Fake {
        address: String,
        taken: Arc<Mutex<usize>>,
        thread: JoinHandle<()>,
    }

    impl Fake {
        /// Waits until the fake took `count` connections.
        fn took(&self, count: usize) {
            let started = Instant::now();
            while *self.taken.lock().expect("counted") < count {
                assert!(
                    started.elapsed() < HANG_GUARD,
                    "fewer connections than {count}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        /// Its thread's end: an assertion it raised fails the test here, with its words.
        fn done(self) {
            if let Err(panic) = self.thread.join() {
                std::panic::resume_unwind(panic);
            }
        }
    }

    /// A fake game server answering its connections in turn with `answers`, each given the
    /// connection; one handed back is read to its end. An answer that keeps it closes it at
    /// once, whatever is unread.
    fn serve(answers: Vec<fn(Peer) -> Option<Peer>>) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("bound").to_string();
        let taken = Arc::new(Mutex::new(0));
        let counted = Arc::clone(&taken);
        let thread = std::thread::spawn(move || {
            for answer in answers {
                let (stream, _) = listener.accept().expect("a connection");
                *counted.lock().expect("counted") += 1;
                if let Some(mut peer) = answer(Peer(stream)) {
                    while peer.recv().is_some() {}
                }
            }
        });
        Fake {
            address,
            taken,
            thread,
        }
    }

    /// Tells the fake server within `secs`.
    fn leave(fake: &Fake, secs: u64) -> Outcome {
        let address = fake.address.clone();
        let connect: Connect = Box::new(move |io| Link::open(Endpoint::parse(&address)?, io));
        let cancelled = AtomicBool::new(false);
        tell(
            connect,
            &hello(),
            Instant::now() + Duration::from_secs(secs),
            &cancelled,
        )
    }

    fn read_hello(peer: &mut Peer) {
        assert!(matches!(peer.recv(), Some(ClientMsg::Hello { .. })));
    }

    /// The welcome, the surrender, then its probe, answered.
    fn admit_and_echo(mut peer: Peer) -> Option<Peer> {
        read_hello(&mut peer);
        peer.send(&welcome());
        surrender_echoed(&mut peer);
        Some(peer)
    }

    /// The surrender and its probe, the probe echoed.
    fn surrender_echoed(peer: &mut Peer) {
        assert_eq!(peer.recv(), Some(ClientMsg::LeaveGame));
        assert_eq!(peer.recv(), Some(ClientMsg::Probe { id: PROBE }));
        peer.send(&ServerMsg::Echo { id: PROBE });
    }

    #[test]
    fn a_seat_still_held_is_asked_for_again_until_it_is_given_back() {
        let fake = serve(vec![
            |mut peer| {
                read_hello(&mut peer);
                peer.send(&ServerMsg::Refused {
                    reason: RejectReason::SeatHeld,
                });
                Some(peer)
            },
            admit_and_echo,
        ]);
        assert_eq!(leave(&fake, 60), Outcome::Left);
        fake.took(2);
        fake.done();
    }

    /// The server pings before it admits: the client answers the ping and says nothing else, so
    /// its next word after the greeting is the pong.
    #[test]
    fn the_surrender_waits_for_the_welcome() {
        let fake = serve(vec![|mut peer| {
            read_hello(&mut peer);
            peer.send(&ServerMsg::Ping);
            assert_eq!(
                peer.recv(),
                Some(ClientMsg::Pong),
                "nothing is said before the seat is admitted"
            );
            peer.send(&welcome());
            surrender_echoed(&mut peer);
            Some(peer)
        }]);
        assert_eq!(leave(&fake, 60), Outcome::Left);
        fake.done();
    }

    #[test]
    fn a_reset_around_the_surrender_says_it_again_on_a_fresh_connection() {
        let fake = serve(vec![
            |mut peer| {
                read_hello(&mut peer);
                peer.send(&welcome());
                assert_eq!(peer.recv(), Some(ClientMsg::LeaveGame));
                // The probe has arrived and is never read: closing with it unread resets the
                // connection, and no echo went back.
                peer.until_more_arrived();
                None
            },
            admit_and_echo,
        ]);
        assert_eq!(leave(&fake, 60), Outcome::Left);
        fake.took(2);
        fake.done();
    }

    #[test]
    fn a_close_before_the_echo_says_it_again_on_a_fresh_connection() {
        let fake = serve(vec![
            |mut peer| {
                read_hello(&mut peer);
                peer.send(&welcome());
                assert_eq!(peer.recv(), Some(ClientMsg::LeaveGame));
                assert_eq!(peer.recv(), Some(ClientMsg::Probe { id: PROBE }));
                // Closed in order, with no echo: the surrender's fate is not known.
                peer.0.shutdown(Shutdown::Write).expect("closed");
                Some(peer)
            },
            admit_and_echo,
        ]);
        assert_eq!(leave(&fake, 60), Outcome::Left);
        fake.took(2);
        fake.done();
    }

    #[test]
    fn a_seat_refused_for_good_is_not_tried_again() {
        let fake = serve(vec![|mut peer| {
            read_hello(&mut peer);
            peer.send(&ServerMsg::Refused {
                reason: RejectReason::NotAdmitted,
            });
            Some(peer)
        }]);
        // The leave's one thread ends with the refusal: no other connection follows it.
        assert_eq!(leave(&fake, 60), Outcome::Refused("NotAdmitted".to_owned()));
        fake.done();
    }

    #[test]
    fn a_game_that_never_answers_is_given_up_at_the_deadline() {
        // Takes the connection and says nothing until the client lets go of it.
        let fake = serve(vec![|mut peer| {
            read_hello(&mut peer);
            Some(peer)
        }]);
        let outcome = leave(&fake, 2);
        assert!(matches!(outcome, Outcome::Unreachable(_)), "{outcome:?}");
        fake.done();
    }

    /// A leave stopped before it starts opens no connection and says nothing.
    #[test]
    fn a_cancelled_leave_opens_no_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("bound").to_string();
        listener.set_nonblocking(true).expect("non-blocking");
        let connect: Connect = Box::new(move |io| Link::open(Endpoint::parse(&address)?, io));
        let cancelled = AtomicBool::new(true);
        let outcome = tell(
            connect,
            &hello(),
            Instant::now() + Duration::from_secs(60),
            &cancelled,
        );
        assert_eq!(outcome, Outcome::Cancelled);
        assert!(
            listener.accept().is_err(),
            "no connection came to the server"
        );
    }

    /// The server closes after the surrender without its echo and is gone by the next try (as
    /// bota_server is when a leave in the picks cancels the match): the leave is done.
    #[test]
    fn a_server_gone_after_closing_on_the_surrender_was_left() {
        let fake = serve(vec![|mut peer| {
            read_hello(&mut peer);
            peer.send(&welcome());
            assert_eq!(peer.recv(), Some(ClientMsg::LeaveGame));
            assert_eq!(peer.recv(), Some(ClientMsg::Probe { id: PROBE }));
            peer.0.shutdown(Shutdown::Write).expect("closed");
            Some(peer)
        }]);
        // One connection only: the fake's listener is gone once it has answered it.
        assert_eq!(leave(&fake, 60), Outcome::Left);
        fake.done();
    }
}
