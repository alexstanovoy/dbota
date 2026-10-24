//! Opening the game a [`Launch`] names: a link to its server, greeted with
//! the seat's ticket (or a name, for a direct game), ready to become the
//! session; for a local lobby the server is started here first. On the
//! desktop the connection is made on a thread of its own so the frame loop
//! never waits on it; a browser's WebSocket never blocks.

use bota_client_core::LiveLink;
use bota_proto::{ClientMsg, Codec, Role};

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
        Launch::Direct {
            name,
            spectate,
            key,
            ..
        } => {
            let role = if *spectate {
                Role::Spectator
            } else {
                Role::Player
            };
            Ok(ClientMsg::hello(
                role,
                name.as_str(),
                key.map(|k| k.0),
                bota_core::WIRE_FINGERPRINT,
            ))
        }
        Launch::Host { name, key, .. } => Ok(ClientMsg::hello(
            Role::Player,
            name.as_str(),
            key.map(|k| k.0),
            bota_core::WIRE_FINGERPRINT,
        )),
        // The shell opens these as files, or as the command line did.
        Launch::Replay(_) | Launch::Given => Err(not_here()),
    }
}

/// A replay and the command line's game are not opened here.
fn not_here() -> std::io::Error {
    std::io::Error::other("opened by the shell, not over the network")
}

/// A game opened: its link, and the game server started for it (a local
/// lobby's), which lives as long as this client holds it.
pub struct Opened {
    pub link: Box<dyn LiveLink>,
    pub server: Option<Hosted>,
}

/// A game server this client started. Ended when dropped: the lobby's host
/// quitting ends its game, as Dota's local host does.
pub struct Hosted(#[cfg(not(target_arch = "wasm32"))] std::process::Child);

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Hosted {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A game being opened.
pub struct Opening {
    #[cfg(not(target_arch = "wasm32"))]
    done: std::sync::mpsc::Receiver<std::io::Result<(bota_client_core::Net, Option<Hosted>)>>,
    #[cfg(target_arch = "wasm32")]
    done: Option<std::io::Result<Box<dyn LiveLink>>>,
}

/// How long a started game server has to say it listens.
#[cfg(not(target_arch = "wasm32"))]
const SERVER_PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);

/// The game server binary: `$BOTA_SERVER`, else `bota_server` beside this
/// client, else on the `PATH`.
#[cfg(not(target_arch = "wasm32"))]
fn server_binary(given: Option<&std::path::Path>) -> std::io::Result<std::path::PathBuf> {
    let name = format!("bota_server{}", std::env::consts::EXE_SUFFIX);
    let named = given.map(std::path::Path::to_path_buf).or_else(|| {
        std::env::var_os("BOTA_SERVER")
            .filter(|p| !p.is_empty())
            .map(Into::into)
    });
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(&name)));
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join(&name));
    named
        .into_iter()
        .chain(beside)
        .chain(on_path)
        .find(|p: &std::path::PathBuf| p.is_file())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no bota_server: put it beside dbota or on the PATH, or name it in BOTA_SERVER",
            )
        })
}

/// Starts a local lobby's game server and waits until it listens: on
/// loopback, or on every address with plain text allowed (the local network
/// is trusted, as Dota's LAN lobby trusts it); mid-only for two players, the
/// whole map for more. It writes its replay, ends when nobody came, and when
/// every seat stayed empty five minutes.
#[cfg(not(target_arch = "wasm32"))]
fn start_server(
    port: u16,
    players: u8,
    lan: bool,
    replay: Option<&std::path::Path>,
    server: Option<&std::path::Path>,
) -> std::io::Result<Hosted> {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    let mut command = Command::new(server_binary(server)?);
    command
        .args([
            "--port",
            &port.to_string(),
            "--players",
            &players.to_string(),
        ])
        .args(["--map", if players <= 2 { "2" } else { "0" }])
        .args(["--abandon-unjoined", "--empty-after-secs", "300"])
        .args(if lan {
            ["--bind", "0.0.0.0", "--insecure-plaintext"].as_slice()
        } else {
            ["--bind", "127.0.0.1"].as_slice()
        })
        .env_remove("BOTA_SEATS")
        .env_remove("BOTA_REFEREE_KEY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = replay {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        command.arg("--replay").arg(path);
    }
    let mut child = command.spawn()?;
    let (said, heard) = std::sync::mpsc::channel();
    // Both are read to their end, so the server never writes into a closed pipe.
    let out = child.stdout.take().map(std::io::BufReader::new);
    let err = child.stderr.take().map(std::io::BufReader::new);
    let said_out = said.clone();
    std::thread::Builder::new()
        .name("dbota-host-out".to_owned())
        .spawn(move || {
            for line in out
                .into_iter()
                .flat_map(BufRead::lines)
                .map_while(Result::ok)
            {
                let _ = said_out.send((true, line));
            }
        })?;
    std::thread::Builder::new()
        .name("dbota-host-err".to_owned())
        .spawn(move || {
            for line in err
                .into_iter()
                .flat_map(BufRead::lines)
                .map_while(Result::ok)
            {
                let _ = said.send((false, line));
            }
        })?;
    let hosted = Hosted(child);
    let deadline = std::time::Instant::now() + SERVER_PATIENCE;
    let mut complaint = String::new();
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match heard.recv_timeout(left) {
            Ok((true, line)) if line.trim() == "READY" => return Ok(hosted),
            Ok((false, line)) => complaint = line,
            Ok(_) => {}
            Err(_) => {
                let why = complaint
                    .strip_prefix("bota_server: ")
                    .unwrap_or(&complaint)
                    .trim();
                return Err(std::io::Error::other(if why.is_empty() {
                    "the game server did not start".to_owned()
                } else {
                    format!("the game server did not start: {why}")
                }));
            }
        }
    }
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
                let opened = (|| -> std::io::Result<(Net, Option<Hosted>)> {
                    let greeting = hello(&launch)?;
                    let server = match &launch {
                        Launch::Host {
                            port,
                            players,
                            lan,
                            replay,
                            server,
                            ..
                        } => Some(start_server(
                            *port,
                            *players,
                            *lan,
                            replay.as_deref(),
                            server.as_deref(),
                        )?),
                        _ => None,
                    };
                    let mut net = match &launch {
                        Launch::Seat(h) | Launch::Watch(h) => {
                            bota_radovid::connect_net(h, &trust.radovid)?
                        }
                        Launch::Direct { address, .. } => Net::open(
                            Endpoint::parse(address)?.with_tls(trust.direct.clone()),
                            Redial::DEFAULT,
                        )?,
                        Launch::Host { port, .. } => Net::open(
                            Endpoint::parse(&format!("127.0.0.1:{port}"))?,
                            Redial::DEFAULT,
                        )?,
                        Launch::Replay(_) | Launch::Given => return Err(not_here()),
                    };
                    Transport::send(&mut net, &greeting)?;
                    Transport::send(&mut net, &ClientMsg::Compress(Codec::Deflate))?;
                    Ok((net, server))
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

    /// The game once it is open, or why it could not be.
    pub fn poll(&mut self) -> Option<std::io::Result<Opened>> {
        use std::sync::mpsc::TryRecvError;
        match self.done.try_recv() {
            Ok(opened) => Some(opened.map(|(net, server)| Opened {
                link: Box::new(net),
                server,
            })),
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
                Launch::Host { .. } => {
                    return Err(std::io::Error::other(
                        "a local lobby needs the desktop client: a browser cannot start a server",
                    ));
                }
                Launch::Replay(_) | Launch::Given => return Err(not_here()),
            };
            link.send(&greeting)?;
            link.send(&ClientMsg::Compress(Codec::Deflate))?;
            Ok(Box::new(link) as Box<dyn LiveLink>)
        })();
        Opening { done: Some(opened) }
    }

    pub fn poll(&mut self) -> Option<std::io::Result<Opened>> {
        self.done
            .take()
            .map(|opened| opened.map(|link| Opened { link, server: None }))
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
