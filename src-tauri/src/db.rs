//! SQLite, owned by the core. The schema follows docs/ARCHITECTURE.md: runs made of jobs, approvals
//! (gates and tool asks) and an append-only activity log. Migrations are numbered by `user_version`.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::Result;

const MIGRATIONS: &[&str] = &[
    // 1: first slice
    r#"
    CREATE TABLE runs (
        id            TEXT PRIMARY KEY,
        workflow_id   TEXT NOT NULL,
        number        INTEGER NOT NULL,
        ticket_key    TEXT NOT NULL,
        ticket_title  TEXT NOT NULL,
        ticket_url    TEXT NOT NULL,
        folder        TEXT NOT NULL,
        started_by    TEXT NOT NULL,
        state         TEXT NOT NULL,
        started_at    TEXT NOT NULL,
        ended_at      TEXT,
        UNIQUE (workflow_id, number)
    );
    CREATE INDEX runs_by_ticket ON runs (ticket_key, started_at DESC);

    CREATE TABLE jobs (
        id              TEXT PRIMARY KEY,
        run_id          TEXT NOT NULL REFERENCES runs (id),
        step_id         TEXT NOT NULL,
        state           TEXT NOT NULL,
        attempt         INTEGER NOT NULL DEFAULT 1,
        sdk_session_id  TEXT,
        output          TEXT,
        cost_usd        REAL,
        started_at      TEXT,
        ended_at        TEXT
    );
    CREATE INDEX jobs_by_run ON jobs (run_id);

    CREATE TABLE messages (
        job_id  TEXT NOT NULL REFERENCES jobs (id),
        seq     INTEGER NOT NULL,
        body    TEXT NOT NULL,
        PRIMARY KEY (job_id, seq)
    );

    CREATE TABLE approvals (
        id          TEXT PRIMARY KEY,
        kind        TEXT NOT NULL CHECK (kind IN ('gate', 'tool')),
        run_id      TEXT NOT NULL REFERENCES runs (id),
        job_id      TEXT REFERENCES jobs (id),
        summary     TEXT NOT NULL,
        state       TEXT NOT NULL CHECK (state IN ('pending', 'approved', 'rejected', 'expired')),
        reason      TEXT,
        decided_by  TEXT,
        created_at  TEXT NOT NULL,
        decided_at  TEXT
    );

    CREATE TABLE activity (
        id      INTEGER PRIMARY KEY AUTOINCREMENT,
        at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
        run_id  TEXT,
        what    TEXT NOT NULL,
        detail  TEXT
    );
    "#,
    // 2: the engine. A run keeps the workflow it started with; jobs cover both kinds of step;
    // tool asks keep what the agent wanted to do
    r#"
    ALTER TABLE runs ADD COLUMN workflow_name TEXT NOT NULL DEFAULT '';
    ALTER TABLE runs ADD COLUMN workflow_json TEXT NOT NULL DEFAULT '{}';
    ALTER TABLE jobs ADD COLUMN kind TEXT NOT NULL DEFAULT 'agent';
    ALTER TABLE jobs ADD COLUMN error TEXT;
    ALTER TABLE approvals ADD COLUMN ask_id TEXT;
    ALTER TABLE approvals ADD COLUMN tool_name TEXT;
    ALTER TABLE approvals ADD COLUMN input TEXT;
    CREATE INDEX approvals_pending ON approvals (state, run_id);
    "#,
    // 3: a log per job (activity rows that name it), and when each agent message arrived
    r#"
    ALTER TABLE activity ADD COLUMN job_id TEXT;
    CREATE INDEX activity_by_job ON activity (job_id);
    ALTER TABLE messages ADD COLUMN at TEXT;
    "#,
    // 4: conditions and loops. A job belongs to a loop item by its scope (`each[2]/`); jobs redone after a
    // rejection or a retry are kept but superseded; `data` is a JSON output, or a loop's items
    r#"
    ALTER TABLE jobs ADD COLUMN scope TEXT NOT NULL DEFAULT '';
    ALTER TABLE jobs ADD COLUMN superseded INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE jobs ADD COLUMN data TEXT;
    CREATE INDEX jobs_current ON jobs (run_id, step_id, scope) WHERE superseded = 0;
    "#,
    // 5: the run's board, shared by its agents and the people following it
    r#"
    CREATE TABLE notes (
        id        INTEGER PRIMARY KEY AUTOINCREMENT,
        run_id    TEXT NOT NULL REFERENCES runs (id),
        job_id    TEXT,
        author    TEXT NOT NULL,
        agent     TEXT,
        addressee TEXT,
        body      TEXT NOT NULL,
        at        TEXT NOT NULL
    );
    CREATE INDEX notes_by_run ON notes (run_id, id);
    "#,
    // 6: an agent asking another agent type: a job of its own, under the asking one
    r#"
    ALTER TABLE jobs ADD COLUMN parent_job TEXT;
    ALTER TABLE jobs ADD COLUMN question TEXT;
    ALTER TABLE jobs ADD COLUMN ask_request TEXT;
    CREATE INDEX jobs_by_parent ON jobs (parent_job);
    "#,
    // 7: team steps. What each member has read of the board, and the members waiting for a note
    r#"
    ALTER TABLE jobs ADD COLUMN seen_note INTEGER NOT NULL DEFAULT 0;
    CREATE TABLE board_waits (
        request_id TEXT PRIMARY KEY,
        job_id     TEXT NOT NULL,
        run_id     TEXT NOT NULL
    );
    "#,
    // 8: what someone asked for, on a run started from a request
    r#"
    ALTER TABLE runs ADD COLUMN request TEXT;
    "#,
];

/// The time as SQLite writes it everywhere: `2026-11-25T09:30:00.123Z`.
pub const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

pub struct Db(pub Mutex<Connection>);

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&conn)?;
        Ok(Self(Mutex::new(conn)))
    }

    /// Appends to the activity log: every side effect writes one row.
    pub fn activity(&self, run_id: Option<&str>, what: &str, detail: Option<&str>) -> Result<()> {
        let conn = self.0.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO activity (run_id, what, detail) VALUES (?1, ?2, ?3)",
            (run_id, what, detail),
        )?;
        Ok(())
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current.max(0) as usize) {
        conn.execute_batch(&format!("BEGIN; {sql}; PRAGMA user_version = {}; COMMIT;", i + 1))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Db;

    #[test]
    fn migrates_once_and_logs_activity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sprocket.sqlite");
        Db::open(&path).unwrap();
        // A second open finds the schema in place and changes nothing
        let db = Db::open(&path).unwrap();
        db.activity(None, "app.started", None).unwrap();
        let conn = db.0.lock().unwrap();
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        let rows: i64 = conn.query_row("SELECT count(*) FROM activity", [], |r| r.get(0)).unwrap();
        assert_eq!((version, rows), (super::MIGRATIONS.len() as i64, 1));
    }
}
