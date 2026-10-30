//! A replay's file name: the day and time it ended (UTC), its mode and its match's id.

use super::index::mode_name;

/// Seconds since 1970 now. A browser has no `SystemTime`; its page clock stands in.
pub fn now() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as u64
    }
}

/// `(year, month, day, hour, minute, second)` of `secs` since 1970, in UTC.
fn civil(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's days-to-civil, for days since 1970-01-01.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day, rest / 3600, rest % 3600 / 60, rest % 60)
}

/// `20261030-153045-solo_mid-4711.brp`; a match without an id (a lobby on this machine) has
/// none in its name.
pub fn file_name(when: u64, mode: &str, match_id: Option<u64>) -> String {
    let (y, mo, d, h, mi, s) = civil(when);
    let id = match_id.map_or_else(String::new, |id| format!("-{id}"));
    format!(
        "{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}-{}{id}.brp",
        mode_name(mode)
    )
}

/// A name no kept replay has: `name` itself, or with `-2`, `-3`... before its extension.
pub fn unused(name: String, taken: impl Fn(&str) -> bool) -> String {
    if !taken(&name) {
        return name;
    }
    let stem = name.trim_end_matches(".brp");
    (2..)
        .map(|n| format!("{stem}-{n}.brp"))
        .find(|candidate| !taken(candidate))
        .unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_tells_the_day_the_mode_and_the_match() {
        // 2026-10-30 15:30:45 UTC.
        assert_eq!(
            file_name(1_793_374_245, "Solo Mid!", Some(4711)),
            "20261030-153045-solomid-4711.brp"
        );
        assert_eq!(file_name(0, "local", None), "19700101-000000-local.brp");
        // A leap day.
        assert_eq!(
            file_name(1_709_164_800, "", None),
            "20240229-000000-game.brp"
        );
        assert_eq!(
            unused("a.brp".into(), |n| n == "a.brp" || n == "a-2.brp"),
            "a-3.brp"
        );
    }
}
