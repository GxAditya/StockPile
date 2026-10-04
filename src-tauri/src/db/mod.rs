pub mod repo;

use crate::error::{AppError, AppResult};
use crate::util;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS products (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT    NOT NULL COLLATE NOCASE,
    sku             TEXT,
    barcode         TEXT,
    category        TEXT,
    unit            TEXT    NOT NULL DEFAULT 'pcs',
    description     TEXT,
    reorder_point   REAL    NOT NULL DEFAULT 0,
    lead_time_days  INTEGER NOT NULL DEFAULT 7,
    created_at      TEXT    NOT NULL,
    updated_at      TEXT    NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_products_name ON products(name COLLATE NOCASE);
CREATE UNIQUE INDEX IF NOT EXISTS idx_products_sku  ON products(sku) WHERE sku IS NOT NULL AND sku <> '';
CREATE INDEX IF NOT EXISTS idx_products_category ON products(category COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS batches (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    product_id    INTEGER NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    batch_no      TEXT    NOT NULL,
    expiry_date   TEXT,
    unit_price    REAL    NOT NULL DEFAULT 0,
    quantity      INTEGER NOT NULL DEFAULT 0 CHECK (quantity >= 0),
    received_at   TEXT    NOT NULL,
    supplier      TEXT,
    notes         TEXT,
    UNIQUE (product_id, batch_no)
);

CREATE INDEX IF NOT EXISTS idx_batches_product ON batches(product_id);
CREATE INDEX IF NOT EXISTS idx_batches_expiry  ON batches(expiry_date) WHERE quantity > 0;
CREATE INDEX IF NOT EXISTS idx_batches_live    ON batches(quantity) WHERE quantity > 0;

CREATE TABLE IF NOT EXISTS ledger (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    ts            TEXT    NOT NULL,
    kind          TEXT    NOT NULL CHECK (kind IN ('restock','sale','adjustment','void')),
    product_id    INTEGER NOT NULL,
    product_name  TEXT    NOT NULL,
    batch_id      INTEGER,
    batch_no      TEXT,
    expiry_date   TEXT,
    qty_delta     INTEGER NOT NULL,
    unit_price    REAL    NOT NULL DEFAULT 0,
    -- `total_value` is a MAGNITUDE, always >= 0, mirroring `qty_delta.abs() * unit_price`.
    -- Direction lives in `qty_delta`, which is signed. Storing the sign in both columns made
    -- every reader a guess and produced negative revenue and negative buyer totals.
    total_value   REAL    NOT NULL DEFAULT 0 CHECK (total_value >= 0),
    buyer_name    TEXT,
    note          TEXT,
    source        TEXT    NOT NULL DEFAULT 'ui',
    actor         TEXT
);

CREATE INDEX IF NOT EXISTS idx_ledger_ts      ON ledger(ts DESC);
CREATE INDEX IF NOT EXISTS idx_ledger_product ON ledger(product_id);
CREATE INDEX IF NOT EXISTS idx_ledger_kind    ON ledger(kind);
CREATE INDEX IF NOT EXISTS idx_ledger_buyer   ON ledger(buyer_name) WHERE kind = 'sale';
CREATE INDEX IF NOT EXISTS idx_ledger_day     ON ledger(substr(ts,1,10));

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Agent conversation memory: 6-turn window then summarised (PRD 8.3).
CREATE TABLE IF NOT EXISTS agent_messages (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    ts         TEXT NOT NULL,
    role       TEXT NOT NULL CHECK (role IN ('user','assistant')),
    content    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_agent_session ON agent_messages(session_id, id);

-- Rollup of consumed/purchased units per product per day, used by the reorder maths.
CREATE TABLE IF NOT EXISTS usage_daily (
    product_id INTEGER NOT NULL,
    day        TEXT    NOT NULL,
    units_out  INTEGER NOT NULL DEFAULT 0,
    units_in   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (product_id, day)
) WITHOUT ROWID;
"#;

const FTS_SCHEMA: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS products_fts USING fts5(
    name, sku, category, description,
    content='products',
    content_rowid='id',
    tokenize='porter unicode61'
);

CREATE TRIGGER IF NOT EXISTS products_fts_ai AFTER INSERT ON products BEGIN
    INSERT INTO products_fts(rowid, name, sku, category, description)
    VALUES (new.id, new.name, new.sku, new.category, new.description);
END;

CREATE TRIGGER IF NOT EXISTS products_fts_ad AFTER DELETE ON products BEGIN
    INSERT INTO products_fts(products_fts, rowid, name, sku, category, description)
    VALUES ('delete', old.id, old.name, old.sku, old.category, old.description);
END;

CREATE TRIGGER IF NOT EXISTS products_fts_au AFTER UPDATE ON products BEGIN
    INSERT INTO products_fts(products_fts, rowid, name, sku, category, description)
    VALUES ('delete', old.id, old.name, old.sku, old.category, old.description);
    INSERT INTO products_fts(rowid, name, sku, category, description)
    VALUES (new.id, new.name, new.sku, new.category, new.description);
END;
"#;

/// Feature probe results, resolved once at startup.
#[derive(Debug, Clone, Copy)]
pub struct Capabilities {
    pub fts5: bool,
}

/// Single-writer connection. Inventory scale does not justify a server database, and a
/// mutex guarantees the ledger/stock transaction pair can never interleave.
pub struct Db {
    conn: Mutex<Connection>,
    caps: Capabilities,
}

static DB_PATH: Lazy<Mutex<PathBuf>> = Lazy::new(|| Mutex::new(PathBuf::new()));

pub fn db_path() -> PathBuf {
    DB_PATH.lock().clone()
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::internal(format!("cannot create {}: {e}", parent.display())))?;
        }
        *DB_PATH.lock() = path.to_path_buf();

        let conn = Connection::open(path)
            .map_err(|e| AppError::internal(format!("cannot open {}: {e}", path.display())))?;
        Self::configure(&conn)?;
        conn.execute_batch(SCHEMA)?;

        // FTS5 is compiled into bundled SQLite; probe once and degrade to LIKE search if absent.
        let fts5 = conn
            .execute_batch(FTS_SCHEMA)
            .is_ok()
            .then(|| conn.query_row("SELECT count(*) FROM products_fts", [], |r| r.get::<_, i64>(0)).is_ok())
            .unwrap_or(false);

        if fts5 {
            // Backfill any rows that predate the index.
            conn.execute(
                "INSERT INTO products_fts(products_fts) VALUES('rebuild')",
                [],
            )?;
        }

        Ok(Self {
            conn: Mutex::new(conn),
            caps: Capabilities { fts5 },
        })
    }

    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        conn.execute_batch(SCHEMA)?;
        let fts5 = conn.execute_batch(FTS_SCHEMA).is_ok();
        Ok(Self {
            conn: Mutex::new(conn),
            caps: Capabilities { fts5 },
        })
    }

    fn configure(conn: &Connection) -> AppResult<()> {
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(())
    }

    pub fn caps(&self) -> Capabilities {
        self.caps
    }

    /// Run `f` with the writer connection inside an IMMEDIATE transaction.
    pub fn write<T>(&self, f: impl FnOnce(&rusqlite::Transaction<'_>) -> AppResult<T>) -> AppResult<T> {
        let mut guard = self.conn.lock();
        let tx = guard.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// Read-only access for analytics. Same connection, no transaction.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let guard = self.conn.lock();
        f(&guard)
    }

    pub fn today() -> String {
        util::today_str()
    }
}

/// Independent read-only connection used by the NL→SQL guard (PRD 8.4).
/// Opened with `SQLITE_OPEN_READ_ONLY` *and* `PRAGMA query_only` so generated SQL
/// physically cannot write even if the validator were bypassed.
pub fn open_read_only(path: &Path) -> AppResult<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| AppError::internal(format!("cannot open read-only: {e}")))?;
    conn.pragma_update(None, "query_only", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(conn)
}