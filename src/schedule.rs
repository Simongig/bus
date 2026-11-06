//! What is happening when: notification texts and the `bus now` status line.

use crate::model::{fmt_duration, fmt_time, Day, Stop};

#[derive(Debug, PartialEq, Eq)]
pub struct Alert {
    pub summary: String,
    pub body: String,
}

/// Alerts for stop starts and ends in the window `(from, to]` (minutes of `day`).
/// A start that directly follows an ending stop is folded into that end alert.
pub fn alerts(day: &Day, from: i32, to: i32) -> Vec<Alert> {
    let in_window = |t: i32| from < t && t <= to;
    let mut out = Vec::new();
    for (i, stop) in day.stops.iter().enumerate() {
        let follows_previous = i > 0 && day.stops[i - 1].end() == stop.start;
        if in_window(stop.start) && !follows_previous {
            out.push(start_alert(stop));
        }
        if in_window(stop.end()) {
            out.push(end_alert(stop, day.stops.get(i + 1)));
        }
    }
    out
}

fn start_alert(stop: &Stop) -> Alert {
    Alert {
        summary: format!("▶ {} · {}", stop.title, fmt_duration(stop.duration)),
        body: start_body(stop),
    }
}

fn start_body(stop: &Stop) -> String {
    let mut body = format!("until {}", fmt_time(stop.end()));
    for item in &stop.dod {
        body.push_str(&format!("\n☐ {}", item.text));
    }
    body
}

fn end_alert(stop: &Stop, next: Option<&Stop>) -> Alert {
    let summary = match next {
        Some(n) if n.start == stop.end() => {
            format!(
                "■ {} done → ▶ {} · {}",
                stop.title,
                n.title,
                fmt_duration(n.duration)
            )
        }
        Some(n) => format!(
            "■ {} done → next: {} {}",
            stop.title,
            n.title,
            fmt_time(n.start)
        ),
        None => format!("■ {} done", stop.title),
    };
    let open: Vec<&str> = stop
        .dod
        .iter()
        .filter(|d| !d.done)
        .map(|d| d.text.as_str())
        .collect();
    let body = if open.is_empty() {
        if stop.dod.is_empty() {
            String::new()
        } else {
            "All done ✓".into()
        }
    } else {
        format!("Still open:\n• {}", open.join("\n• "))
    };
    Alert { summary, body }
}

/// One line for tmux or a status bar, e.g. `Deep work 42m → Lunch 12:30`.
///
/// `format` may use `{title}`, `{left}`, `{next}`, `{next_start}` and `{next_in}`;
/// placeholders without a value become empty.
pub fn status_line(day: &Day, minute: i32, format: Option<&str>) -> String {
    let (current, next) = day.current_and_next(minute);
    let title = current.map(|s| s.title.clone()).unwrap_or_default();
    let left = current
        .map(|s| fmt_duration(s.end() - minute))
        .unwrap_or_default();
    let next_title = next.map(|s| s.title.clone()).unwrap_or_default();
    let next_start = next.map(|s| fmt_time(s.start)).unwrap_or_default();
    let next_in = next
        .map(|s| fmt_duration(s.start - minute))
        .unwrap_or_default();

    if let Some(format) = format {
        return format
            .replace("{title}", &title)
            .replace("{left}", &left)
            .replace("{next}", &next_title)
            .replace("{next_start}", &next_start)
            .replace("{next_in}", &next_in);
    }
    let head = match (current, next) {
        (Some(_), _) => format!("{title} {left}"),
        (None, Some(_)) => format!("free {next_in}"),
        (None, None) => "free".into(),
    };
    match next {
        Some(_) => format!("{head} → {next_title} {next_start}"),
        None => head,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DodItem;
    use chrono::NaiveDate;

    fn day() -> Day {
        let mut day = Day::new(NaiveDate::from_ymd_opt(2026, 11, 4).unwrap());
        let mut work = Stop::new("Deep work", 540, 90);
        work.dod = vec![
            DodItem {
                text: "draft".into(),
                done: true,
            },
            DodItem {
                text: "review".into(),
                done: false,
            },
        ];
        day.insert(work).unwrap();
        day.insert(Stop::new("Break", 630, 15)).unwrap();
        day.insert(Stop::new("Lunch", 750, 60)).unwrap();
        day
    }

    #[test]
    fn start_alert_fires_inside_window_only() {
        let d = day();
        assert_eq!(alerts(&d, 538, 539), vec![]);
        let a = alerts(&d, 539, 540);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].summary, "▶ Deep work · 1h30m");
        assert_eq!(a[0].body, "until 10:30\n☐ draft\n☐ review");
    }

    #[test]
    fn back_to_back_stops_give_one_combined_alert() {
        let a = alerts(&day(), 629, 630);
        assert_eq!(
            a,
            vec![Alert {
                summary: "■ Deep work done → ▶ Break · 15m".into(),
                body: "Still open:\n• review".into(),
            }]
        );
    }

    #[test]
    fn end_before_a_gap_names_next_stop() {
        let a = alerts(&day(), 644, 645);
        assert_eq!(a[0].summary, "■ Break done → next: Lunch 12:30");
    }

    #[test]
    fn status_lines() {
        let d = day();
        assert_eq!(status_line(&d, 588, None), "Deep work 42m → Break 10:30");
        assert_eq!(status_line(&d, 700, None), "free 50m → Lunch 12:30");
        assert_eq!(status_line(&d, 780, None), "Lunch 30m");
        assert_eq!(status_line(&d, 900, None), "free");
        assert_eq!(
            status_line(&d, 588, Some("[{title}|{left}]")),
            "[Deep work|42m]"
        );
    }
}
