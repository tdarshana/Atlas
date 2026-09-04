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
"#), (3, r#"
create table if not exists tasks (
  id uuid primary key,
  key text not null unique,
  project_id uuid,
  seq bigint not null,
  title text not null,
  description text not null default '',
  stage text not null,
  kind text not null check (kind in ('task','bug','feature','chore')),
  priority text not null check (priority in ('low','medium','high','urgent')),
  assignee text,
  labels json not null default '[]',
  parent_id uuid,
  created_by text not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  closed_at timestamptz);
create table if not exists task_blockers (
  task_id uuid not null, blocked_by uuid not null, primary key (task_id, blocked_by));
create table if not exists task_events (
  id uuid primary key, task_id uuid not null, actor text not null, kind text not null,
  body text not null default '', detail json,
  created_at timestamptz not null default now());
create table if not exists board_counters (scope text primary key, next_seq bigint not null);
alter table projects add column if not exists board_key text;
alter table projects add column if not exists board_stages json;
"#), (4, r#"
-- `board_counters` was added to migration 3 after an earlier build had already
-- stamped databases at version 3 without it. `migrate` runs a migration only when
-- its number is above the stored version, so those databases would never see that
-- statement again and every task create would fail on a missing table. This is a
-- no-op on a correct database and repairs a wrong one.
create table if not exists board_counters (scope text primary key, next_seq bigint not null);
"#), (5, r#"
-- Phase 8 widens `projects` in place, the same `if not exists` shape migration 3
-- used for `board_key`, so a re-run on a database that already has the columns is
-- a no-op rather than an error.
alter table projects add column if not exists agent_access json;
alter table projects add column if not exists extraction json;
"#), (6, r#"
-- Phase 9 gives the name `workflows` to real, graph-shaped workflows. The table
-- migration 1 created under that name held Markdown documents, so it is renamed and
-- `library::DocRepo` follows it. `workflow::migrate_docs` turns each surviving row
-- into a single-action workflow at daemon start and then empties the table; it is
-- kept rather than dropped so that migration can read it. The rename itself is done
-- in `rename_workflow_docs` rather than here, because DuckDB has no
-- `alter table if exists` and replaying this batch has to stay a no-op.
create table if not exists workflows (
  id uuid primary key,
  name text not null unique,
  project_id uuid,
  description text not null default '',
  "trigger" json not null,
  graph json not null,
  enabled boolean not null default true,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  last_run_at timestamptz,
  last_status text);
create table if not exists workflow_runs (
  id uuid primary key,
  workflow_id uuid not null,
  number bigint not null,
  "trigger" text not null check ("trigger" in ('manual','schedule','prompt')),
  status text not null default 'queued' check (status in ('queued','running','success','failed','cancelled')),
  started_at timestamptz not null default now(),
  finished_at timestamptz,
  summary json);
create table if not exists workflow_steps (
  id uuid primary key,
  run_id uuid not null,
  "position" integer not null,
  action_id text not null,
  name text not null,
  agent text not null,
  status text not null default 'running' check (status in ('queued','running','success','failed','skipped','cancelled')),
  started_at timestamptz not null default now(),
  finished_at timestamptz,
  output text,
  log json not null default '[]');
"#), (7, r#"
-- Phase 12 links an imported task back to the framework file it came from, the
-- same `if not exists` shape migration 3 and 5 use for a column added to a
-- table already carrying rows.
alter table tasks add column if not exists source_ref json;
"#)];

/// Moves the Markdown workflow documents aside so migration 6 can give the name
/// `workflows` to the real workflow table. Guarded on `body`, a column only the doc
/// table has, so replaying migration 6 over an already-migrated database does nothing
/// instead of failing on a name that is already taken.
fn rename_workflow_docs(c: &Connection) -> Result<()> {
    let is_doc_table: i64 = c.query_row(
        "select count(*) from information_schema.columns where table_name = 'workflows' and column_name = 'body'",
        [],
        |r| r.get(0),
    )?;
    if is_doc_table == 1 {
        c.execute_batch("alter table workflows rename to workflow_docs;")?;
    }
    Ok(())
}

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
                if *v == 6 { rename_workflow_docs(c)?; }
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
        assert_eq!(db.schema_version().unwrap(), 7);
        let n: i64 = db.with_conn(|c| Ok(c.query_row(
            "select count(*) from information_schema.tables where table_name in ('memories','memory_embeddings','audit','settings','projects','agents','practices','workflow_docs','sync_targets','jobs','tasks','task_blockers','task_events','board_counters','workflows','workflow_runs','workflow_steps')",
            [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 17);
        // Migrations 3 and 5 widen `projects` in place.
        let cols: i64 = db.with_conn(|c| Ok(c.query_row(
            "select count(*) from information_schema.columns where table_name='projects' and column_name in ('board_key','board_stages','agent_access','extraction')",
            [], |r| r.get(0))?)).unwrap();
        assert_eq!(cols, 4);
        db.migrate().unwrap(); // second run is a no-op
        assert_eq!(db.schema_version().unwrap(), 7);
    }

    /// Migration 6 renames the Markdown doc table out of the way and puts the real
    /// workflow tables in its place, so a database that predates Phase 9 keeps its
    /// workflow documents where `DocRepo` can still read them.
    #[test]
    fn migration_6_moves_the_workflow_docs_aside() {
        let db = Db::open_in_memory().unwrap();
        let cols: i64 = db
            .with_conn(|c| {
                Ok(c.query_row(
                    "select count(*) from information_schema.columns where table_name = 'workflows' and column_name in ('trigger','graph','enabled','last_run_at','last_status')",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(cols, 5);
        let body: i64 = db
            .with_conn(|c| {
                Ok(c.query_row(
                    "select count(*) from information_schema.columns where table_name = 'workflow_docs' and column_name = 'body'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(body, 1);
    }

    /// Migration 5 adds its columns with `if not exists`, so replaying it over a
    /// database that already has them is a no-op rather than a failure.
    #[test]
    fn migration_5_is_a_no_op_on_a_database_that_already_has_the_columns() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn(|c| {
            c.execute_batch("delete from schema_version where version >= 5;")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.schema_version().unwrap(), 4);
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 7);
    }

    /// A database stamped 3 by the build that shipped migration 3 without
    /// `board_counters` gets the table from migration 4 rather than failing every
    /// task create on a missing table.
    #[test]
    fn migration_4_adds_board_counters_to_a_v3_database() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn(|c| {
            c.execute_batch("drop table board_counters; delete from schema_version where version >= 4;")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.schema_version().unwrap(), 3);

        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 7);
        let n: i64 = db
            .with_conn(|c| {
                Ok(c.query_row("select count(*) from information_schema.tables where table_name = 'board_counters'", [], |r| r.get(0))?)
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn open_on_disk_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.duckdb");
        let _db = Db::open(&p).unwrap();
        assert!(p.exists());
    }
}
