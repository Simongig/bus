//! Domain model: days, stops and the ripple rules for re-blocking.
//!
//! All times are minutes since midnight on the stop's day.

use chrono::NaiveDate;

/// All start times and durations snap to this grid (minutes).
pub const GRID: i32 = 15;
/// Minutes in a day; no stop may end after this.
pub const DAY_END: i32 = 24 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DodItem {
    pub text: String,
    pub done: bool,
}

/// A time block: one "stop" on the day's route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    pub title: String,
    pub start: i32,
    pub duration: i32,
    /// Definition of done.
    pub dod: Vec<DodItem>,
}

impl Stop {
    pub fn new(title: impl Into<String>, start: i32, duration: i32) -> Self {
        Self {
            title: title.into(),
            start,
            duration,
            dod: Vec::new(),
        }
    }

    pub fn end(&self) -> i32 {
        self.start + self.duration
    }

    pub fn contains(&self, minute: i32) -> bool {
        self.start <= minute && minute < self.end()
    }
}

/// One planned day. `stops` is always sorted by start and never overlaps.
///
/// Every mutating method is atomic: on `Err` the day is left unchanged.
/// On `Ok` it returns the index of the affected stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Day {
    pub date: NaiveDate,
    pub stops: Vec<Stop>,
}

impl Day {
    pub fn new(date: NaiveDate) -> Self {
        Self {
            date,
            stops: Vec::new(),
        }
    }

    /// Inserts a stop and pushes any later stops it overlaps.
    /// A stop starting at the same time as an existing one goes before it.
    pub fn insert(&mut self, stop: Stop) -> Result<usize, String> {
        validate(&stop)?;
        let index = self.stops.partition_point(|s| s.start < stop.start);
        if let Some(prev) = index.checked_sub(1).map(|i| &self.stops[i]) {
            if prev.end() > stop.start {
                return Err(format!(
                    "Overlaps \"{}\" ({}–{})",
                    prev.title,
                    fmt_time(prev.start),
                    fmt_time(prev.end())
                ));
            }
        }
        let mut stops = self.stops.clone();
        stops.insert(index, stop);
        ripple(&mut stops, index)?;
        self.stops = stops;
        Ok(index)
    }

    pub fn replace(&mut self, index: usize, stop: Stop) -> Result<usize, String> {
        let mut next = self.clone();
        next.stops.remove(index);
        let index = next.insert(stop)?;
        *self = next;
        Ok(index)
    }

    pub fn remove(&mut self, index: usize) -> Stop {
        self.stops.remove(index)
    }

    /// Moves a stop. Later moves push overlapped stops; earlier moves never pull
    /// other stops and are blocked by the previous stop.
    pub fn move_by(&mut self, index: usize, delta: i32) -> Result<usize, String> {
        let mut stops = self.stops.clone();
        let start = stops[index].start + delta;
        if start < 0 {
            return Err("Can't start before 00:00".into());
        }
        if let Some(prev) = index.checked_sub(1).map(|i| &stops[i]) {
            if prev.end() > start {
                return Err(format!("Blocked by \"{}\"", prev.title));
            }
        }
        stops[index].start = start;
        ripple(&mut stops, index)?;
        self.stops = stops;
        Ok(index)
    }

    pub fn resize_by(&mut self, index: usize, delta: i32) -> Result<usize, String> {
        let mut stops = self.stops.clone();
        let duration = stops[index].duration + delta;
        if duration < GRID {
            return Err(format!("Minimum length is {GRID}m"));
        }
        stops[index].duration = duration;
        ripple(&mut stops, index)?;
        self.stops = stops;
        Ok(index)
    }

    /// The stop running at `minute` and the first one starting after it.
    pub fn current_and_next(&self, minute: i32) -> (Option<&Stop>, Option<&Stop>) {
        let current = self.stops.iter().find(|s| s.contains(minute));
        let next = self.stops.iter().find(|s| s.start > minute);
        (current, next)
    }

    pub fn planned_minutes(&self) -> i32 {
        self.stops.iter().map(|s| s.duration).sum()
    }
}

fn validate(stop: &Stop) -> Result<(), String> {
    if stop.title.trim().is_empty() {
        return Err("Title is empty".into());
    }
    if stop.start % GRID != 0 || stop.duration % GRID != 0 {
        return Err(format!("Times must be on the {GRID}-minute grid"));
    }
    if stop.duration < GRID {
        return Err(format!("Minimum length is {GRID}m"));
    }
    if stop.start < 0 || stop.end() > DAY_END {
        return Err("A stop must fit between 00:00 and 24:00".into());
    }
    Ok(())
}

/// Pushes the stops after `from` later until nothing overlaps.
/// A free gap absorbs the shift and ends the ripple.
fn ripple(stops: &mut [Stop], from: usize) -> Result<(), String> {
    for i in from + 1..stops.len() {
        let prev_end = stops[i - 1].end();
        if stops[i].start >= prev_end {
            break;
        }
        stops[i].start = prev_end;
    }
    match stops.iter().find(|s| s.end() > DAY_END) {
        Some(s) => Err(format!("Would push \"{}\" past midnight", s.title)),
        None => Ok(()),
    }
}

fn snap(minutes: i32) -> i32 {
    (minutes + GRID / 2) / GRID * GRID
}

/// Rounds up to the next grid slot (unchanged if already on the grid).
pub fn ceil_to_grid(minutes: i32) -> i32 {
    (minutes + GRID - 1) / GRID * GRID
}

/// Parses `9:30`, `09:30`, `930`, `0930` or `9`, snapped to the grid.
pub fn parse_time(input: &str) -> Result<i32, String> {
    let s = input.trim();
    let err = || format!("Can't read time \"{s}\" (try 9:30 or 0930)");
    if !s.is_ascii() {
        return Err(err());
    }
    let (h, m) = match s.split_once(':') {
        Some(parts) => parts,
        None if s.len() <= 2 => (s, "0"),
        None if s.len() <= 4 => s.split_at(s.len() - 2),
        None => return Err(err()),
    };
    let h: i32 = h.parse().map_err(|_| err())?;
    let m: i32 = m.parse().map_err(|_| err())?;
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return Err(err());
    }
    let minutes = snap(h * 60 + m);
    if minutes >= DAY_END {
        return Err(format!("{s} rounds to midnight; pick an earlier time"));
    }
    Ok(minutes)
}

/// Parses `90`, `90m`, `1h`, `1h30`, `1h30m` or `1.5h`, snapped to the grid.
pub fn parse_duration(input: &str) -> Result<i32, String> {
    let s: String = input
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let err = || {
        format!(
            "Can't read duration \"{}\" (try 45m, 1h30 or 90)",
            input.trim()
        )
    };
    let minutes: f64 = match s.split_once('h') {
        Some((h, rest)) => {
            let hours: f64 = h.parse().map_err(|_| err())?;
            let rest = rest.strip_suffix('m').unwrap_or(rest);
            let mins: f64 = if rest.is_empty() {
                0.0
            } else {
                rest.parse().map_err(|_| err())?
            };
            hours * 60.0 + mins
        }
        None => s
            .strip_suffix('m')
            .unwrap_or(&s)
            .parse()
            .map_err(|_| err())?,
    };
    if !minutes.is_finite() || minutes <= 0.0 || minutes > DAY_END as f64 {
        return Err(err());
    }
    Ok(((minutes / GRID as f64).round() as i32 * GRID).max(GRID))
}

/// `09:30`
pub fn fmt_time(minutes: i32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// `45m`, `2h`, `1h30m`
pub fn fmt_duration(minutes: i32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h{m}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(stops: &[(&str, i32, i32)]) -> Day {
        let mut day = Day::new(NaiveDate::from_ymd_opt(2026, 11, 4).unwrap());
        for &(title, start, duration) in stops {
            day.insert(Stop::new(title, start, duration)).unwrap();
        }
        day
    }

    fn starts(day: &Day) -> Vec<(String, String)> {
        day.stops
            .iter()
            .map(|s| (s.title.clone(), fmt_time(s.start)))
            .collect()
    }

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn moving_later_ripples_until_a_gap_absorbs_it() {
        let mut d = day(&[("A", 540, 60), ("B", 600, 60), ("C", 720, 60)]);
        d.move_by(0, 30).unwrap();
        assert_eq!(
            starts(&d),
            pairs(&[("A", "09:30"), ("B", "10:30"), ("C", "12:00")])
        );
        d.move_by(0, 60).unwrap();
        assert_eq!(
            starts(&d),
            pairs(&[("A", "10:30"), ("B", "11:30"), ("C", "12:30")])
        );
    }

    #[test]
    fn moving_earlier_never_pulls_and_is_blocked_by_previous() {
        let mut d = day(&[("A", 540, 60), ("B", 600, 60)]);
        assert_eq!(d.move_by(1, -15), Err("Blocked by \"A\"".into()));
        d.move_by(1, 30).unwrap();
        d.move_by(1, -15).unwrap();
        assert_eq!(starts(&d), pairs(&[("A", "09:00"), ("B", "10:15")]));
    }

    #[test]
    fn growing_ripples_and_shrinking_does_not_pull() {
        let mut d = day(&[("A", 540, 60), ("B", 600, 60)]);
        d.resize_by(0, 15).unwrap();
        assert_eq!(starts(&d), pairs(&[("A", "09:00"), ("B", "10:15")]));
        d.resize_by(0, -30).unwrap();
        assert_eq!(starts(&d), pairs(&[("A", "09:00"), ("B", "10:15")]));
        assert!(d.resize_by(0, -45).is_err());
    }

    #[test]
    fn ripple_past_midnight_fails_and_leaves_day_unchanged() {
        let mut d = day(&[("A", 1320, 60), ("B", 1380, 60)]);
        let before = d.clone();
        assert_eq!(
            d.move_by(0, 15),
            Err("Would push \"B\" past midnight".into())
        );
        assert_eq!(d, before);
    }

    #[test]
    fn insert_at_same_start_goes_first_and_pushes() {
        let mut d = day(&[("A", 540, 60)]);
        assert_eq!(d.insert(Stop::new("Standup", 540, 15)), Ok(0));
        assert_eq!(starts(&d), pairs(&[("Standup", "09:00"), ("A", "09:15")]));
    }

    #[test]
    fn insert_inside_previous_stop_is_rejected() {
        let mut d = day(&[("A", 540, 60)]);
        assert!(d.insert(Stop::new("B", 570, 30)).is_err());
    }

    #[test]
    fn replace_can_reorder() {
        let mut d = day(&[("A", 540, 60), ("B", 600, 60)]);
        assert_eq!(d.replace(0, Stop::new("A", 720, 60)), Ok(1));
        assert_eq!(starts(&d), pairs(&[("B", "10:00"), ("A", "12:00")]));
    }

    #[test]
    fn current_and_next() {
        let d = day(&[("A", 540, 60), ("B", 660, 60)]);
        let (cur, next) = d.current_and_next(560);
        assert_eq!(cur.map(|s| s.title.as_str()), Some("A"));
        assert_eq!(next.map(|s| s.title.as_str()), Some("B"));
        assert_eq!(d.current_and_next(610).0, None);
    }

    #[test]
    fn parses_times() {
        assert_eq!(parse_time("9:30"), Ok(570));
        assert_eq!(parse_time("0930"), Ok(570));
        assert_eq!(parse_time("930"), Ok(570));
        assert_eq!(parse_time("9"), Ok(540));
        assert_eq!(parse_time("9:08"), Ok(555));
        assert!(parse_time("24:00").is_err());
        assert!(parse_time("23:55").is_err());
        assert!(parse_time("x").is_err());
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("90"), Ok(90));
        assert_eq!(parse_duration("45m"), Ok(45));
        assert_eq!(parse_duration("1h"), Ok(60));
        assert_eq!(parse_duration("1h30"), Ok(90));
        assert_eq!(parse_duration("1h 30m"), Ok(90));
        assert_eq!(parse_duration("1.5h"), Ok(90));
        assert_eq!(parse_duration("5"), Ok(15));
        assert!(parse_duration("0").is_err());
        assert!(parse_duration("h").is_err());
    }

    #[test]
    fn formats() {
        assert_eq!(fmt_time(570), "09:30");
        assert_eq!(fmt_duration(45), "45m");
        assert_eq!(fmt_duration(120), "2h");
        assert_eq!(fmt_duration(90), "1h30m");
    }
}
