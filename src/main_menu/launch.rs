//! Opening the game a [`Launch`] names: a link to its server, greeted with
//! the seat's ticket (or a name, for a direct game), ready to become the
//! session. On the desktop the connection is made on a thread of its own so
//! the frame loop never waits on it; a browser's WebSocket never blocks.

use bota_client_core::LiveLink;
use bota_proto::{ClientMsg, Role};

use super::core::Launch;

/// Which certificates `wss://` servers and the service must show.
#[derive(Clone)]
pub struct Trust {
    /// The service and the hosted games.
    pub radovid: bota_radovid::Tls,
    /// Direct `wss://` games.
    #[cfg(not(target_arch = "wasm32"))]
    pub direct: bota_client_core::TlsConfig,
}

impl Trust {
    /// The public web's roots, or exactly the certificates of a PEM file
    /// (a development service).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(pem: Option<&str>) -> std::io::Result<Trust> {
        Ok(match pem {
            Some(pem) => Trust {
                radovid: bota_radovid::Tls::trust_pem(pem)?,
                direct: bota_client_core::TlsConfig::trust_pem(pem)?,
            },
            None => Trust {
                radovid: bota_radovid::Tls::webpki(),
                direct: bota_client_core::TlsConfig::webpki(),
            },
        })
    }

    /// The browser trusts what it trusts.
    #[cfg(target_arch = "wasm32")]
    pub fn new(_pem: Option<&str>) -> std::io::Result<Trust> {
        Ok(Trust {
            radovid: bota_radovid::Tls::webpki(),
        })
    }
}

/// The first message to a game server.
fn hello(launch: &Launch) -> std::io::Result<ClientMsg> {
    match launch {
        Launch::Seat(handoff) => bota_radovid::seat_hello(handoff, Role::Player),
        Launch::Watch(handoff) => bota_radovid::seat_hello(handoff, Role::Spectator),
        Launch::Direct { name, spectate, .. } => {
            let role = if *spectate {
                Role::Spectator
            } else {
                Role::Player
            };
            Ok(ClientMsg::hello(role, name.as_str(), None))
        }
    }
}

/// A game being opened.
pub struct Opening {
    #[cfg(not(target_arch = "wasm32"))]
    done: std::sync::mpsc::Receiver<std::io::Result<bota_client_core::Net>>,
    #[cfg(target_arch = "wasm32")]
    done: Option<std::io::Result<Box<dyn LiveLink>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Opening {
    pub fn start(launch: Launch, trust: &Trust) -> Opening {
        use bota_client_core::{Endpoint, Net, Redial, Transport};
        let (tell, done) = std::sync::mpsc::sync_channel(1);
        let trust = trust.clone();
        let spawned = std::thread::Builder::new()
            .name("dbota-join".to_owned())
            .spawn(move || {
                let opened = (|| -> std::io::Result<Net> {
                    let greeting = hello(&launch)?;
                    let mut net = match &launch {
                        Launch::Seat(h) | Launch::Watch(h) => {
                            bota_radovid::connect_net(h, &trust.radovid)?
                        }
                        Launch::Direct { address, .. } => Net::open(
                            Endpoint::parse(address)?.with_tls(trust.direct.clone()),
                            Redial::DEFAULT,
                        )?,
                    };
                    Transport::send(&mut net, &greeting)?;
                    Ok(net)
                })();
                let _ = tell.send(opened);
            });
        if let Err(err) = spawned {
            let (tell, done) = std::sync::mpsc::sync_channel(1);
            let _ = tell.send(Err(err));
            return Opening { done };
        }
        Opening { done }
    }

    /// The link once it is open, or why it could not be.
    pub fn poll(&mut self) -> Option<std::io::Result<Box<dyn LiveLink>>> {
        use std::sync::mpsc::TryRecvError;
        match self.done.try_recv() {
            Ok(opened) => Some(opened.map(|net| Box::new(net) as Box<dyn LiveLink>)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err(std::io::Error::other("the connection attempt died")))
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Opening {
    pub fn start(launch: Launch, _trust: &Trust) -> Opening {
        let opened = (|| -> std::io::Result<Box<dyn LiveLink>> {
            let greeting = hello(&launch)?;
            let mut link = match &launch {
                Launch::Seat(h) | Launch::Watch(h) => bota_radovid::web_link(h)?,
                Launch::Direct { address, .. } => {
                    bota_client_core::WebSocketLink::connect(address)?
                }
            };
            link.send(&greeting)?;
            Ok(Box::new(link) as Box<dyn LiveLink>)
        })();
        Opening { done: Some(opened) }
    }

    pub fn poll(&mut self) -> Option<std::io::Result<Box<dyn LiveLink>>> {
        self.done.take()
    }
}

/// The link of the menu while no game is on screen: nothing comes and
/// nothing goes.
pub struct Idle;

impl LiveLink for Idle {
    fn poll(&mut self) -> Vec<bota_proto::ServerMsg> {
        Vec::new()
    }

    fn send(&mut self, _msg: &ClientMsg) -> std::io::Result<()> {
        Ok(())
    }

    fn is_closed(&self) -> bool {
        false
    }
}
