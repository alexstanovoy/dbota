//! The menu's remembered choices and the saved login, as text the store
//! keeps.

use bota_radovid::SessionToken;

/// Longest value kept.
const MAX_VALUE: usize = 256;
/// Hosts remembered as told about plain TCP, at most.
pub const MAX_PLAIN_HOSTS: usize = 32;

/// What the menu remembers between runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    /// The matchmaking service, `wss://host:port`.
    pub service: String,
    /// The last login typed (never the password).
    pub login: String,
    /// The name a direct game announces.
    pub name: String,
    /// The last direct address.
    pub direct: String,
    /// Hosts the player was told plain TCP is unprotected for, and went on.
    pub plain_ok: Vec<String>,
}

impl Prefs {
    /// Reads `name = value` lines; anything else is skipped.
    pub fn parse(text: &str) -> Prefs {
        let mut prefs = Prefs::default();
        for line in text.lines().take(256) {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            if value.len() > MAX_VALUE || value.chars().any(char::is_control) {
                continue;
            }
            let value = value.to_owned();
            match name.trim() {
                "service" => prefs.service = value,
                "login" => prefs.login = value,
                "name" => prefs.name = value,
                "direct" => prefs.direct = value,
                "plain_ok" if prefs.plain_ok.len() < MAX_PLAIN_HOSTS => prefs.plain_ok.push(value),
                _ => {}
            }
        }
        prefs
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for (name, value) in [
            ("service", &self.service),
            ("login", &self.login),
            ("name", &self.name),
            ("direct", &self.direct),
        ] {
            if !value.is_empty() {
                out.push_str(&format!("{name} = {value}\n"));
            }
        }
        for host in &self.plain_ok {
            out.push_str(&format!("plain_ok = {host}\n"));
        }
        out
    }

    /// Remembers that plain TCP to `host` is the player's choice; the oldest
    /// host goes when the list is full.
    pub fn allow_plain(&mut self, host: &str) {
        if self.plain_ok.iter().any(|h| h == host) {
            return;
        }
        if self.plain_ok.len() >= MAX_PLAIN_HOSTS {
            self.plain_ok.remove(0);
        }
        self.plain_ok.push(host.to_owned());
    }
}

/// The token of a login, for the service it came from.
#[derive(Clone, PartialEq, Eq)]
pub struct Saved {
    pub service: String,
    pub token: SessionToken,
}

impl Saved {
    pub fn parse(text: &str) -> Option<Saved> {
        let mut lines = text.lines();
        let service = lines.next()?.trim().to_owned();
        let hex = lines.next()?.trim();
        if service.len() > MAX_VALUE || hex.len() != 64 {
            return None;
        }
        let mut token = [0u8; 32];
        for (byte, pair) in token.iter_mut().zip(hex.as_bytes().chunks(2)) {
            *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
        }
        Some(Saved {
            service,
            token: SessionToken(token),
        })
    }

    pub fn render(&self) -> String {
        let hex: String = self.token.0.iter().map(|b| format!("{b:02x}")).collect();
        format!("{}\n{hex}\n", self.service)
    }
}

/// The service of the page a browser build runs in: the page's own host, on
/// the service's control port. Nothing on the desktop.
pub fn page_service() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let host = web_sys::window()?.location().hostname().ok()?;
        (!host.is_empty()).then(|| format!("wss://{host}:{SERVICE_PORT}"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// The control port the hosted service listens on.
#[cfg(target_arch = "wasm32")]
const SERVICE_PORT: u16 = 4460;
