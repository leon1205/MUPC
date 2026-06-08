use rusqlite::{Connection, Result as SqliteResult};
use std::path::PathBuf;

pub fn init_database(db_path: &PathBuf) -> SqliteResult<Connection> {
    let conn = Connection::open(db_path)?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS fault_records (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            fault_type TEXT NOT NULL,
            trigger_time INTEGER NOT NULL,
            over_voltage REAL,
            under_voltage REAL,
            over_current REAL,
            frequency_abnormal REAL,
            waveform_path TEXT
        )",
        [],
    )?;

    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_trigger_time ON fault_records(trigger_time)",
        [],
    )?;

    Ok(conn)
}
