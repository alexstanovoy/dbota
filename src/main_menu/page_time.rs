//! A render clock: live local time in the app, fixed UTC only for explicit screenshot fixtures.
use super::view::PageView;

#[derive(Clone, Copy)]
pub struct Clock {
    pub now: u64,
    pub preview: bool,
}
impl Clock {
    pub const PREVIEW: u64 = 1793997600; // 2026-11-06 20:40 UTC.
    pub fn new(preview: bool) -> Self {
        Self {
            preview,
            now: if preview {
                Self::PREVIEW
            } else {
                crate::replays::now()
            },
        }
    }
    /// The minute a page is drawn for. Only the pages that write a date or "min ago" change with
    /// it; redrawing any other would drop the hover, focus and animation of what the player is on.
    pub fn minute_shown(self, body: &PageView) -> u64 {
        match body {
            PageView::Home { .. } | PageView::Profile { .. } => self.now / 60,
            _ => 0,
        }
    }
    fn full_date(self, when: u64) -> String {
        if self.preview {
            i64::try_from(when)
                .ok()
                .and_then(|seconds| jiff::Timestamp::from_second(seconds).ok())
                .map_or_else(
                    || "Unknown date".to_owned(),
                    |time| {
                        time.to_zoned(jiff::tz::TimeZone::UTC)
                            .strftime("%Y-%m-%d %H:%M")
                            .to_string()
                    },
                )
        } else {
            std::time::UNIX_EPOCH
                .checked_add(std::time::Duration::from_secs(when))
                .map_or_else(|| "Unknown date".to_owned(), super::replay_info::when)
        }
    }
    pub fn date(self, when: u64) -> String {
        let date = self.full_date(when);
        let today = self.full_date(self.now);
        let (Some(day), Some(today_day)) = (date.get(..10), today.get(..10)) else {
            return date;
        };
        let Ok(day) = day.parse::<jiff::civil::Date>() else {
            return date;
        };
        let Ok(today) = today_day.parse::<jiff::civil::Date>() else {
            return date;
        };
        let label = if day == today {
            "Today".to_owned()
        } else if today.yesterday().ok() == Some(day) {
            "Yesterday".to_owned()
        } else {
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            format!("{} {}", day.day(), MONTHS[(day.month() - 1) as usize])
        };
        format!("{label}\n{}", date.get(11..16).unwrap_or(""))
    }
    pub fn ago(self, when: u64) -> String {
        let seconds = self.now.saturating_sub(when);
        if seconds < 60 {
            "Just now".to_owned()
        } else if seconds < 3600 {
            format!("{} min ago", seconds / 60)
        } else if seconds < 86_400 {
            format!("{} h ago", seconds / 3600)
        } else {
            format!("{} d ago", seconds / 86_400)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_damaged_history_timestamp_does_not_panic_the_menu() {
        let clock = Clock {
            now: Clock::PREVIEW,
            preview: false,
        };
        assert!(clock.date(u64::MAX).to_lowercase().contains("unknown"));
    }
}
