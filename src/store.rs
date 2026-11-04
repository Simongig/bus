//! Persistence. Everything goes through [`Store`] so another backend
//! (e.g. libSQL or an HTTP API) can replace SQLite without touching the UI.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::{params, Connection};

use crate::model::{Day, DodItem, Stop};

pub trait Store {
    /// Returns an empty day if nothing is planned for `date`.
    fn load_day(&self, date: NaiveDate) -> Result<Day>;
    /// Replaces everything stored for `day.date`.
    fn save_day(&mut self, day: &Day) -> Result<()>;
}

pub struct SqliteStore {
    conn: Connection,
}

const SCHEMA_V1: &str = "
    CREATE TABLE stops (
        id           INTEGER PRIMARY KEY,
        date         TEXT    NOT NULL,
        start_min    INTEGER NOT NULL,
        duration_min INTEGER NOT NULL,
        title        TEXT    NOT NULL
    );
    CREATE INDEX stops_date ON stops(date);
    CREATE TABLE dod_items (
        stop_id  INTEGER NOT NULL REFERENCES stops(id) ON DELETE CASCADE,
        position INTEGER NOT NULL,
        text     TEXT    NOT NULL,
        done     INTEGER NOT NULL
    );
    CREATE INDEX dod_items_stop ON dod_items(stop_id);
";

impl SqliteStore {
    /// `$BUS_DB` if set, otherwise `~/.local/share/bus/bus.db`.
    pub fn default_path() -> Result<PathBuf> {
        if let Some(path) = std::env::var_os("BUS_DB") {
            return Ok(path.into());
        }
        let dir = dirs::data_dir().context("Can't find a data directory ($XDG_DATA_HOME or ~/.local/share)")?;
        Ok(dir.join("bus").join("bus.db"))
    }

    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("Can't create {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("Can't open {}", path.display()))?;
        // WAL lets `bus now` read while the TUI writes.
        conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(2))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(&format!("BEGIN; {SCHEMA_V1} PRAGMA user_version = 1; COMMIT;"))?;
        }
        Ok(Self { conn })
    }
}

impl Store for SqliteStore {
    fn load_day(&self, date: NaiveDate) -> Result<Day> {
        let mut stops_stmt = self
            .conn
            .prepare_cached("SELECT id, start_min, duration_min, title FROM stops WHERE date = ?1 ORDER BY start_min")?;
        let rows = stops_stmt
            .query_map(params![date.to_string()], |r| {
                Ok((r.get::<_, i64>(0)?, Stop::new(r.get::<_, String>(3)?, r.get(1)?, r.get(2)?)))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut items_stmt =
            self.conn.prepare_cached("SELECT text, done FROM dod_items WHERE stop_id = ?1 ORDER BY position")?;
        let mut day = Day::new(date);
        for (id, mut stop) in rows {
            stop.dod = items_stmt
                .query_map(params![id], |r| Ok(DodItem { text: r.get(0)?, done: r.get(1)? }))?
                .collect::<rusqlite::Result<_>>()?;
            day.stops.push(stop);
        }
        Ok(day)
    }

    fn save_day(&mut self, day: &Day) -> Result<()> {
        let date = day.date.to_string();
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM stops WHERE date = ?1", params![date])?;
        for stop in &day.stops {
            tx.execute(
                "INSERT INTO stops (date, start_min, duration_min, title) VALUES (?1, ?2, ?3, ?4)",
                params![date, stop.start, stop.duration, stop.title],
            )?;
            let id = tx.last_insert_rowid();
            for (position, item) in stop.dod.iter().enumerate() {
                tx.execute(
                    "INSERT INTO dod_items (stop_id, position, text, done) VALUES (?1, ?2, ?3, ?4)",
                    params![id, position as i64, item.text, item.done],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
    }

    #[test]
    fn round_trips_a_day_and_keeps_other_days() {
        let mut store = SqliteStore::in_memory().unwrap();
        let mut day = Day::new(date(4));
        let mut stop = Stop::new("Deep work", 540, 90);
        stop.dod = vec![
            DodItem { text: "draft".into(), done: true },
            DodItem { text: "review".into(), done: false },
        ];
        day.insert(stop).unwrap();
        day.insert(Stop::new("Lunch", 720, 60)).unwrap();
        store.save_day(&day).unwrap();

        let mut other = Day::new(date(5));
        other.insert(Stop::new("Gym", 420, 60)).unwrap();
        store.save_day(&other).unwrap();

        assert_eq!(store.load_day(date(4)).unwrap(), day);
        assert_eq!(store.load_day(date(5)).unwrap(), other);
        assert_eq!(store.load_day(date(6)).unwrap(), Day::new(date(6)));
    }

    #[test]
    fn save_replaces_the_day() {
        let mut store = SqliteStore::in_memory().unwrap();
        let mut day = Day::new(date(4));
        day.insert(Stop::new("A", 540, 60)).unwrap();
        store.save_day(&day).unwrap();
        day.remove(0);
        store.save_day(&day).unwrap();
        assert!(store.load_day(date(4)).unwrap().stops.is_empty());
    }
}
