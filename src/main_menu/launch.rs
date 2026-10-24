//! Opening the game a [`Launch`] names: a link to its server, greeted with
//! the seat's ticket (or a name, for a direct game), ready to become the
//! session; for a local lobby the server is started here first. On the
//! desktop the connection is made on a thread of its own so the frame loop
//! never waits on it; a browser's WebSocket never blocks.

use bota_client_core::LiveLink;
use bota_proto::{ClientMsg, Codec, Role};

use super::core::Launch;
#[cfg(not(target_arch = "wasm32"))]
use super::core::{Claim, Fill};

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
        Launch::Host { name, key, .. } | Launch::Demo { name, key, .. } => Ok(ClientMsg::hello(
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

/// A game server this client started, and the bots it started for the
/// seats left to them. Ended when dropped: the lobby's host quitting ends
/// its game, as Dota's local host does.
pub struct Hosted {
    #[cfg(not(target_arch = "wasm32"))]
    server: std::process::Child,
    #[cfg(not(target_arch = "wasm32"))]
    bots: Vec<std::process::Child>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Hosted {
    fn drop(&mut self) {
        for child in self.bots.iter_mut().chain([&mut self.server]) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// The words of a bot command, as a shell would split them but run by
/// none: blanks part words, '…' keeps what it holds, "…" too with \" and
/// \\ inside. `{address}` in a word becomes the game's address.
pub fn command_words(line: &str, address: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("a ' is not closed".to_owned()),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => word.push(c),
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err("a \" is not closed".to_owned()),
                        },
                        Some(c) => word.push(c),
                        None => return Err("a \" is not closed".to_owned()),
                    }
                }
            }
            c => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        words.push(word);
    }
    if words.is_empty() {
        return Err("the bot command is empty".to_owned());
    }
    Ok(words
        .into_iter()
        .map(|w| w.replace("{address}", address))
        .collect())
}

/// Starts the bot command once for each of `seats`, each with a seat key of
/// its own in `BOTA_SEAT_KEY` (never an argument: arguments show in process
/// lists), its output on this client's terminal.
#[cfg(not(target_arch = "wasm32"))]
fn start_bots(
    command: &str,
    address: &str,
    seats: u8,
) -> std::io::Result<Vec<std::process::Child>> {
    use std::process::{Command, Stdio};
    let words = command_words(command, address)
        .map_err(|why| std::io::Error::other(format!("the bot command: {why}")))?;
    let mut bots = Vec::new();
    for _ in 0..seats {
        let mut bot = Command::new(&words[0]);
        bot.args(&words[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        if let Some(Claim(key)) = Claim::draw() {
            bot.env("BOTA_SEAT_KEY", key.to_hex());
        }
        let started = bot.spawn().map_err(|why| {
            std::io::Error::other(format!(
                "the bot command did not start ({}): {why}",
                words[0]
            ))
        })?;
        bots.push(started);
    }
    Ok(bots)
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

/// What a local lobby's game server is started with: on loopback, or on
/// every address with plain text allowed (the local network is trusted, as
/// Dota's LAN lobby trusts it); mid-only for two players, the whole map for
/// more; the empty seats as its host fills them; cheats when its host
/// enables them. It ends when nobody came, and when every seat stayed empty
/// five minutes.
#[cfg(not(target_arch = "wasm32"))]
pub fn host_args(port: u16, players: u8, lan: bool, fill: &Fill, cheats: bool) -> Vec<String> {
    let mut args: Vec<String> = [
        "--port",
        &port.to_string(),
        "--players",
        &players.to_string(),
        "--map",
        if players <= 2 { "2" } else { "0" },
        "--abandon-unjoined",
        "--empty-after-secs",
        "300",
    ]
    .map(str::to_owned)
    .into();
    if *fill == Fill::Dummy {
        args.push("--dummy-seat".to_owned());
    }
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

/// What Dota's Demo Hero is started with: one seat on our map, on this
/// machine alone and a port of its own, cheats on, the hero chosen the only
/// one to pick, and no planning before the game.
#[cfg(not(target_arch = "wasm32"))]
pub fn demo_args(hero: u16) -> Vec<String> {
    [
        "--port",
        "0",
        "--bind",
        "127.0.0.1",
        "--players",
        "1",
        "--map",
        "0",
        "--cheats",
        "--tactical-secs",
        "0",
        "--heroes",
        &hero.to_string(),
        "--abandon-unjoined",
        "--empty-after-secs",
        "60",
    ]
    .map(str::to_owned)
    .into()
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
    let hosted = Hosted {
        server: child,
        bots: Vec::new(),
    };
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
                            players,
                            lan,
                            replay,
                            server,
                            fill,
                            cheats,
                            ..
                        } => Some(start_server(
                            &host_args(*port, *players, *lan, fill, *cheats),
                            replay.as_deref(),
                            server.as_deref(),
                        )?),
                        Launch::Demo { hero, server, .. } => {
                            Some(start_server(&demo_args(hero.0), None, server.as_deref())?)
                        }
                        _ => None,
                    };
                    let (server, listening) = match started {
                        Some((server, listening)) => (Some(server), Some(listening)),
                        None => (None, None),
                    };
                    let mut net = match &launch {
                        Launch::Seat(h) | Launch::Watch(h) => {
                            bota_radovid::connect_net(h, &trust.radovid)?
                        }
                        Launch::Direct { address, .. } => Net::open(
                            Endpoint::parse(address)?.with_tls(trust.direct.clone()),
                            Redial::DEFAULT,
                        )?,
                        Launch::Host { .. } | Launch::Demo { .. } => Net::open(
                            Endpoint::parse(&listening.unwrap_or_default())?,
                            Redial::DEFAULT,
                        )?,
                        Launch::Replay(_) | Launch::Given => return Err(not_here()),
                    };
                    Transport::send(&mut net, &greeting)?;
                    Transport::send(&mut net, &ClientMsg::Compress(Codec::Deflate))?;
                    // The bots come once the host has asked for its seat, so
                    // the host keeps the first one.
                    let mut server = server;
                    if let (
                        Launch::Host {
                            port,
                            players,
                            fill: Fill::Bot(command),
                            ..
                        },
                        Some(hosted),
                    ) = (&launch, server.as_mut())
                    {
                        hosted.bots = start_bots(
                            command,
                            &format!("127.0.0.1:{port}"),
                            players.saturating_sub(1),
                        )?;
                    }
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
                Launch::Host { .. } | Launch::Demo { .. } => {
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::command_words;

    /// A bot command splits as a shell would, runs by none, and takes the
    /// game's address where it asks for it.
    #[test]
    fn bot_commands_split_into_words() {
        let at = "127.0.0.1:4455";
        let words = |line: &str| command_words(line, at);
        assert_eq!(
            words("bot --addr {address}  --name 'Bot One'"),
            Ok(vec!["bot", "--addr", at, "--name", "Bot One"]
                .into_iter()
                .map(String::from)
                .collect())
        );
        assert_eq!(
            words(r#""/opt/my bot/run" "say \"hi\" \\ {address}" ''"#),
            Ok(vec![
                "/opt/my bot/run".to_owned(),
                format!(r#"say "hi" \ {at}"#),
                String::new()
            ])
        );
        assert_eq!(
            words("a;b|c $(d)"),
            Ok(vec!["a;b|c".to_owned(), "$(d)".to_owned()])
        );
        assert!(words("   ").is_err());
        assert!(words("bot 'open").is_err());
        assert!(words("bot \"open").is_err());
    }
}
