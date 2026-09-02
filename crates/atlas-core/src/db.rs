use std::path::Path;
use std::sync::Mutex;
use duckdb::Connection;
use crate::Result;

pub struct Db { conn: Mutex<Connection> }

const MIGRATIONS: &[(i64, &str)] = &[(1, r#"
create table if not exists schema_version (version bigint not null);
create table if not exists projects (
  id uuid primary key, name text not null, root_path text not null, git_remote text,
  profile json, created_at timestamp not null default now(), last_seen_at timestamp not null default now());
create table if not exists memories (
  id uuid primary key,
  scope text not null check (scope in ('global','project')),
  project_id uuid,
  kind text not null check (kind in ('fact','decision','preference','insight','todo')),
  text text not null,
  tags text[] not null default [],
  source_agent text, source_tool text,
  confidence double not null default 1.0,
  status text not null default 'active' check (status in ('active','pending','rejected','superseded')),
  superseded_by uuid,
  created_at timestamp not null default now(),
  updated_at timestamp not null default now());
create table if not exists memory_embeddings (memory_id uuid primary key, model text not null, vector float[]);
create table if not exists agents (
  id uuid primary key, name text not null unique, description text not null, instructions text not null,
  model_hint text, tools text[] not null default [], tags text[] not null default [], version integer not null default 1,
  created_at timestamp not null default now(), updated_at timestamp not null default now());
create table if not exists practices (
  id uuid primary key, name text not null unique, body text not null, tags text[] not null default [], project_id uuid,
  created_at timestamp not null default now(), updated_at timestamp not null default now());
create table if not exists workflows (
  id uuid primary key, name text not null unique, body text not null, tags text[] not null default [], project_id uuid,
  created_at timestamp not null default now(), updated_at timestamp not null default now());
create table if not exists settings (key text primary key, value json not null);
create table if not exists audit (
  id uuid primary key, "at" timestamp not null default now(), actor text not null, action text not null,
  entity text not null, entity_id uuid, detail json);
create table if not exists sync_targets (
  id uuid primary key, project_id uuid, kind text not null check (kind in ('claude','codex','agents_md')),
  path text not null, last_synced_at timestamp);
"#), (2, r#"
create table if not exists jobs (
  id uuid primary key, kind text not null,
  status text not null default 'queued' check (status in ('queued','running','done','failed')),
  payload json, result json, error text,
  created_at timestamp not null default now(), updated_at timestamp not null default now());
"#)];

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        let db = Db { conn: Mutex::new(Connection::open(path)?) };
        db.migrate()?; Ok(db)
    }
    pub fn open_in_memory() -> Result<Db> {
        let db = Db { conn: Mutex::new(Connection::open_in_memory()?) };
        db.migrate()?; Ok(db)
    }
    /// Poison-tolerant, like the lock accessors in `MemoryService`: a panic raised
    /// while the connection was held must not turn every later query in the daemon
    /// into a "poisoned lock" error. DuckDB itself is unharmed by a panic in the
    /// closure, so recovering the guard is safe.
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&guard)
    }
    pub fn schema_version(&self) -> Result<i64> {
        self.with_conn(|c| {
            let exists: i64 = c.query_row("select count(*) from information_schema.tables where table_name='schema_version'", [], |r| r.get(0))?;
            if exists == 0 { return Ok(0); }
            Ok(c.query_row("select coalesce(max(version),0) from schema_version", [], |r| r.get(0))?)
        })
    }
    pub fn migrate(&self) -> Result<()> {
        let current = self.schema_version()?;
        self.with_conn(|c| {
            for (v, sql) in MIGRATIONS { if *v > current {
                c.execute_batch(sql)?;
                c.execute("insert into schema_version values (?)", [v])?;
            } }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migrate_creates_tables_and_is_idempotent() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), 2);
        let n: i64 = db.with_conn(|c| Ok(c.query_row(
            "select count(*) from information_schema.tables where table_name in ('memories','memory_embeddings','audit','settings','projects','agents','practices','workflows','sync_targets','jobs')",
            [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 10);
        db.migrate().unwrap(); // second run is a no-op
        assert_eq!(db.schema_version().unwrap(), 2);
    }
    #[test]
    fn open_on_disk_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.duckdb");
        let _db = Db::open(&p).unwrap();
        assert!(p.exists());
    }
}
