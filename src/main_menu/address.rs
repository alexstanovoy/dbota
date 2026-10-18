//! What a direct address may be on this platform.

/// Whether `host` is this machine.
fn loopback(host: &str) -> bool {
    host == "localhost" || host.starts_with("127.") || host == "[::1]"
}

/// The host of `host:port`, `[v6]:port` or a URL's authority.
fn host_of(rest: &str) -> &str {
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.starts_with('[') {
        return authority
            .find(']')
            .map_or(authority, |end| &authority[..=end]);
    }
    authority.split(':').next().unwrap_or(authority)
}

/// Checks a direct address: `host:port` or `tcp://host:port` for TCP on the
/// desktop, `ws://` or `wss://` anywhere (in a browser `ws://` only to this
/// machine: a page may not open plain connections elsewhere). The host when
/// the connection would be unprotected and leave the machine, so the player
/// can be told; nothing for a protected or local one.
pub fn check(address: &str) -> Result<Option<String>, &'static str> {
    if address.is_empty() {
        return Err("Type an address: host:port, tcp://host:port, ws:// or wss://");
    }
    if address.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("An address has no spaces");
    }
    if address.starts_with("wss://") {
        return Ok(None);
    }
    let (rest, websocket) = match address.strip_prefix("ws://") {
        Some(rest) => (rest, true),
        None => (address.strip_prefix("tcp://").unwrap_or(address), false),
    };
    if !websocket && cfg!(target_arch = "wasm32") {
        return Err("A browser cannot open TCP: use wss://host:port");
    }
    let host = host_of(rest);
    let port = rest[host.len()..].split('/').next().unwrap_or("");
    if host.is_empty()
        || port
            .strip_prefix(':')
            .is_none_or(|p| p.parse::<u16>().is_err())
    {
        return Err("The address needs a host and a port: host:port");
    }
    if loopback(host) {
        return Ok(None);
    }
    if websocket && cfg!(target_arch = "wasm32") {
        return Err("A page can open ws:// only to this machine: use wss://");
    }
    Ok(Some(host.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn plain_connections_off_this_machine_are_named_for_the_notice() {
        assert_eq!(check("127.0.0.1:4455"), Ok(None));
        assert_eq!(check("tcp://[::1]:4455"), Ok(None));
        assert_eq!(check("wss://game.example.org:4456"), Ok(None));
        assert_eq!(
            check("192.168.1.5:4455"),
            Ok(Some("192.168.1.5".to_owned()))
        );
        assert_eq!(check("ws://lan-box:4456/x"), Ok(Some("lan-box".to_owned())));
        assert!(check("host").is_err());
        assert!(check("host:99999").is_err());
        assert!(check("").is_err());
    }
}
