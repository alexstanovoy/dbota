//! Opening the game a [`Launch`] names: a link to its server, greeted with
//! the seat's ticket (or a name, for a direct game), ready to become the
//! session; for a local lobby the server is started here first. On the
//! desktop the connection is made on a thread of its own so the frame loop
//! never waits on it; a browser's WebSocket never blocks.

use bota_client_core::LiveLink;
use bota_proto::{ClientMsg, Codec, Role};

use super::core::Launch;
#[cfg(not(target_arch = "wasm32"))]
use super::core::{LOCAL_SEATS, LocalRules};

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
        Launch::Seat(handoff) | Launch::Demo { handoff, .. } => {
            bota_radovid::seat_hello(handoff, Role::Player)
        }
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
        Launch::Replay(_) | Launch::Blob(_) | Launch::Given => Err(not_here()),
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

/// A game server this client started. Ended when dropped: the lobby's host quitting ends its
/// game, as Dota's local host does.
pub struct Hosted {
    #[cfg(not(target_arch = "wasm32"))]
    server: std::process::Child,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Hosted {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
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

/// The game server binary, as this client's own wire: the first of these that is a file and
/// speaks the wire this client speaks, in this order: `bota_server` beside this client (what
/// `tools/build.sh` makes), the one named in the settings or `$BOTA_SERVER`, and one on the
/// `PATH`. A server of another wire is skipped, and said so if no other is found.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn server_binary(
    given: Option<&std::path::Path>,
) -> std::io::Result<std::path::PathBuf> {
    let name = format!("bota_server{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(&name)));
    let named = given.map(std::path::Path::to_path_buf).or_else(|| {
        std::env::var_os("BOTA_SERVER")
            .filter(|p| !p.is_empty())
            .map(Into::into)
    });
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join(&name));
    let ours = bota_radovid::fingerprint();
    let mut other_wires = Vec::new();
    for candidate in beside.into_iter().chain(named).chain(on_path) {
        if !candidate.is_file() {
            continue;
        }
        match wire_of(&candidate) {
            Some(theirs) if theirs == ours => return Ok(candidate),
            theirs => other_wires.push((candidate, theirs)),
        }
    }
    let found = other_wires
        .iter()
        .map(|(path, wire)| match wire {
            Some(wire) => format!("{} speaks wire {wire:016x}", path.display()),
            None => format!("{} did not say its wire", path.display()),
        })
        .collect::<Vec<_>>();
    let found = if found.is_empty() {
        String::new()
    } else {
        format!(" (found, and not usable: {})", found.join("; "))
    };
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!(
            "no bota_server of this client's wire {ours:016x}{found}: build it beside dbota with \
             tools/build.sh, or name it in BOTA_SERVER"
        ),
    ))
}

/// The wire a game server binary speaks, from its own `--wire-fingerprint`.
#[cfg(not(target_arch = "wasm32"))]
fn wire_of(server: &std::path::Path) -> Option<u64> {
    let output = std::process::Command::new(server)
        .arg("--wire-fingerprint")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    u64::from_str_radix(String::from_utf8_lossy(&output.stdout).trim(), 16).ok()
}

/// What a local lobby's game server is started with: on loopback, or on every address with
/// plain text allowed (the local network is trusted, as Dota's LAN lobby trusts it); the
/// rules' map and the lobby's ten seats under any rules; the match starting with the seats
/// held and ready, nothing in the empty ones; cheats when its host enables them. It ends when
/// nobody came, and when every seat stayed empty five minutes.
#[cfg(not(target_arch = "wasm32"))]
pub fn host_args(port: u16, rules: LocalRules, lan: bool, cheats: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "--port",
        &port.to_string(),
        "--players",
        &LOCAL_SEATS.to_string(),
        "--seated-start",
        "--map",
        &rules.map().0.to_string(),
        "--abandon-unjoined",
        "--empty-after-secs",
        "300",
    ]
    .map(str::to_owned)
    .into();
    if lan {
        args.extend(["--bind", "0.0.0.0", "--insecure-plaintext"].map(str::to_owned));
    } else {
        args.extend(["--bind", "127.0.0.1"].map(str::to_owned));
    }
    if cheats {
        args.push("--cheats".to_owned());
    }
    args
}

/// Starts a game server with `args` and waits until it listens: the server,
/// and the plain address it said it listens on.
#[cfg(not(target_arch = "wasm32"))]
fn start_server(
    args: &[String],
    replay: Option<&std::path::Path>,
    server: Option<&std::path::Path>,
) -> std::io::Result<(Hosted, String)> {
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    let mut command = Command::new(server_binary(server)?);
    command
        .args(args)
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
    let hosted = Hosted { server: child };
    let deadline = std::time::Instant::now() + SERVER_PATIENCE;
    let mut complaint = String::new();
    let mut listening = None;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match heard.recv_timeout(left) {
            Ok((true, line)) if line.trim() == "READY" => {
                let address = listening
                    .ok_or_else(|| std::io::Error::other("the game server named no address"))?;
                return Ok((hosted, address));
            }
            Ok((true, line)) => {
                // The plain address; a WebSocket one is told with its scheme.
                if let Some(address) = line.trim().strip_prefix("bota_server listening on ")
                    && !address.contains("://")
                    && listening.is_none()
                {
                    listening = Some(address.to_owned());
                }
            }
            Ok((false, line)) => complaint = line,
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
                    let started = match &launch {
                        Launch::Host {
                            port,
                            lan,
                            replay,
                            server,
                            cheats,
                            rules,
                            ..
                        } => Some(start_server(
                            &host_args(*port, *rules, *lan, *cheats),
                            replay.as_deref(),
                            server.as_deref(),
                        )?),
                        _ => None,
                    };
                    let (server, listening) = match started {
                        Some((server, listening)) => (Some(server), Some(listening)),
                        None => (None, None),
                    };
                    let mut net = match &launch {
                        Launch::Seat(h) | Launch::Watch(h) | Launch::Demo { handoff: h, .. } => {
                            bota_radovid::connect_net(h, &trust.radovid)?
                        }
                        Launch::Direct { address, .. } => Net::open(
                            Endpoint::parse(address)?.with_tls(trust.direct.clone()),
                            Redial::DEFAULT,
                        )?,
                        Launch::Host { .. } => Net::open(
                            Endpoint::parse(&listening.unwrap_or_default())?,
                            Redial::DEFAULT,
                        )?,
                        Launch::Replay(_) | Launch::Blob(_) | Launch::Given => {
                            return Err(not_here());
                        }
                    };
                    Transport::send(&mut net, &greeting)?;
                    Transport::send(&mut net, &bota_client_core::catalogue())?;
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
                Launch::Seat(h) | Launch::Watch(h) | Launch::Demo { handoff: h, .. } => {
                    bota_radovid::web_link(h)?
                }
                Launch::Direct { address, .. } => {
                    bota_client_core::WebSocketLink::connect(address)?
                }
                Launch::Host { .. } => {
                    return Err(std::io::Error::other(
                        "a local lobby needs the desktop client: a browser cannot start a server",
                    ));
                }
                Launch::Replay(_) | Launch::Blob(_) | Launch::Given => return Err(not_here()),
            };
            link.send(&greeting)?;
            link.send(&bota_client_core::catalogue())?;
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

/// This machine's address on the network its traffic leaves by, which the
/// others of a LAN lobby type. A UDP socket aimed at a documentation-range
/// address (192.0.2.1, never reached: aiming sends nothing) tells it.
#[cfg(not(target_arch = "wasm32"))]
pub fn lan_address() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(("192.0.2.1", 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

/// A browser hosts nothing.
#[cfg(target_arch = "wasm32")]
pub fn lan_address() -> Option<std::net::IpAddr> {
    None
}
