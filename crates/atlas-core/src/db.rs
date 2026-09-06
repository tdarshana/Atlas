use std::path::Path;
use std::sync::Mutex;
use duckdb::Connection;
use tokio::sync::broadcast;
use crate::models::Change;
use crate::Result;

/// How many changes a slow subscriber may fall behind before it is told it lagged;
/// a lagged subscriber refreshes wholesale, so nothing is lost, only coalesced.
const CHANGE_BUFFER: usize = 256;

pub struct Db {
    conn: Mutex<Connection>,
    /// Every write announces itself here (see `Change`); the daemon's event stream
    /// subscribes. A send with no subscriber is not an error.
    changes: broadcast::Sender<Change>,
    /// How many times `with_conn` has run, so a test can pin the query count of a
    /// hot path such as `SettingsRepo::get_all`.
    #[cfg(test)]
    conn_uses: std::sync::atomic::AtomicUsize,
}

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
"#), (8, r#"
-- Task MCP-A: a project can disable MCP tools on top of the global
-- `mcp.disabled_tools` list, the same `if not exists` shape migration 3, 5 and 7
-- use for a column added to a table already carrying rows. Null means "no
-- project override"; `Project::mcp_disabled_tools` reads that as an empty list.
alter table projects add column if not exists mcp_disabled_tools json;
"#), (9, r#"
-- Phase 15: Atlas-native skills, alongside the `SKILL.md` folders discovery finds on
-- disk, and a per-project list of skill ids switched off, the same `if not exists`
-- shape migration 8 uses. Null `skills_disabled` means "no project override";
-- `Project::skills_disabled` reads that as an empty list.
create table if not exists skills (
  id uuid primary key,
  project_id uuid,
  name text not null,
  description text not null default '',
  body text not null default '',
  created_at timestamp not null default now(),
  updated_at timestamp not null default now());
alter table projects add column if not exists skills_disabled json;
"#), (10, r#"
-- PERF-2 (ATL-304): `next_queued` runs `where status = 'queued'` on every worker
-- wake, so the status column gets an index. `if not exists` keeps a replay over a
-- database that already carries it a no-op, the same shape the column migrations use.
create index if not exists jobs_status_idx on jobs (status);
"#), (11, r#"
-- Phase 17: the persona library, each project's roster, and the persona a task is
-- done as. `if not exists` throughout, so a replay over a database that already
-- carries them is a no-op, the same shape migrations 9 and 10 use.
create table if not exists personas (
  id uuid primary key, name text not null, slug text not null,
  role text not null default '', summary text not null default '',
  instructions text not null default '',
  skills text[] not null default [], workflows text[] not null default [],
  practices text[] not null default [], mcp_servers text[] not null default [],
  tools text[] not null default [], access json not null default '{}',
  models json not null default '{}', tags text[] not null default [],
  created_at timestamp not null default now(), updated_at timestamp not null default now());
create unique index if not exists personas_slug_idx on personas (slug);
create table if not exists project_personas (
  project_id uuid not null, persona_id uuid not null,
  is_default boolean not null default false, position integer not null default 0,
  primary key (project_id, persona_id));
alter table tasks add column if not exists persona_id uuid;
"#), (12, r#"
-- PERF-6 (ATL-308): every task detail read and the task-event part of global search
-- filter `task_events` on `task_id`, and the table grows one row per task write with
-- nothing pruning it. `if not exists`, the same shape as migration 10.
create index if not exists task_events_task_id_idx on task_events (task_id);
"#), (13, r#"
-- Task kinds grow (epic, feature_request; ATL-365). DuckDB cannot drop a check
-- constraint, so the table is rebuilt without the kind check: `TaskKind` validates
-- every write, which a text check only duplicated. Column list spelled out on both
-- sides so the copy does not depend on the order later migrations appended columns.
create table tasks_v13 (
  id uuid primary key,
  key text not null unique,
  project_id uuid,
  seq bigint not null,
  title text not null,
  description text not null default '',
  stage text not null,
  kind text not null,
  priority text not null check (priority in ('low','medium','high','urgent')),
  assignee text,
  labels json not null default '[]',
  parent_id uuid,
  created_by text not null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  closed_at timestamptz,
  source_ref json,
  persona_id uuid);
insert into tasks_v13 (id, key, project_id, seq, title, description, stage, kind, priority, assignee, labels, parent_id, created_by, created_at, updated_at, closed_at, source_ref, persona_id)
  select id, key, project_id, seq, title, description, stage, kind, priority, assignee, labels, parent_id, created_by, created_at, updated_at, closed_at, source_ref, persona_id from tasks;
drop table tasks;
alter table tasks_v13 rename to tasks;
"#), (14, r#"
-- Board order (ATL-429): a card's place in its column, backfilled from seq so nothing
-- moves until someone drags.
alter table tasks add column position double;
update tasks set position = seq;
"#), (15, r#"
-- ATL-427: the agent roles table predates personas, which took the name agent on
-- 2026-09-07. Nothing reads it any more.
drop table if exists agents;
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
        let db = Db { conn: Mutex::new(Connection::open(path)?), changes: broadcast::channel(CHANGE_BUFFER).0, #[cfg(test)] conn_uses: Default::default() };
        db.migrate()?;
        // A migration's DDL must never sit in the write-ahead log: DuckDB has refused
        // to replay a log holding `alter table ... add column` after an unclean stop,
        // which lost every write since the previous checkpoint. Folding the migration
        // into the file at once means a crash right after start still reopens.
        db.checkpoint()?;
        Ok(db)
    }
    /// Folds the write-ahead log into the database file. Called after migrations and
    /// on shutdown; cheap when there is nothing to fold.
    pub fn checkpoint(&self) -> Result<()> {
        self.with_conn(|c| { c.execute_batch("checkpoint")?; Ok(()) })
    }
    pub fn open_in_memory() -> Result<Db> {
        let db = Db { conn: Mutex::new(Connection::open_in_memory()?), changes: broadcast::channel(CHANGE_BUFFER).0, #[cfg(test)] conn_uses: Default::default() };
        db.migrate()?; Ok(db)
    }
    /// A receiver of every change written from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<Change> { self.changes.subscribe() }
    /// Announces a write. Called by the two writers every write goes through (task
    /// events and audit rows), while the connection is still held, so a subscriber
    /// that reads back on hearing it waits for the write to be visible.
    pub fn notify(&self, change: Change) { let _ = self.changes.send(change); }
    /// Poison-tolerant, like the lock accessors in `MemoryService`: a panic raised
    /// while the connection was held must not turn every later query in the daemon
    /// into a "poisoned lock" error. DuckDB itself is unharmed by a panic in the
    /// closure, so recovering the guard is safe.
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        #[cfg(test)]
        self.conn_uses.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&guard)
    }
    /// The number of `with_conn` calls so far.
    #[cfg(test)]
    pub(crate) fn conn_uses(&self) -> usize { self.conn_uses.load(std::sync::atomic::Ordering::Relaxed) }
    pub fn schema_version(&self) -> Result<i64> {
        self.with_conn(|c| {
            let exists: i64 = c.query_row("select count(*) from information_schema.tables where table_name='schema_version'", [], |r| r.get(0))?;
            if exists == 0 { return Ok(0); }
            Ok(c.query_row("select coalesce(max(version),0) from schema_version", [], |r| r.get(0))?)
        })
    }
    #[cfg(test)]
    pub(crate) fn open_without_checkpoint(path: &Path) -> Result<Db> {
        let db = Db { conn: Mutex::new(Connection::open(path)?), changes: broadcast::channel(CHANGE_BUFFER).0, #[cfg(test)] conn_uses: Default::default() };
        db.migrate()?;
        Ok(db)
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

    fn wal_len(path: &std::path::Path) -> u64 {
        let wal = path.with_extension("duckdb.wal");
        std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0)
    }

    /// `open` leaves no migration in the write-ahead log: a fresh database is
    /// checkpointed the moment its migrations ran, so a crash right after start
    /// reopens cleanly, whereas migrating alone leaves the DDL in the log.
    #[test]
    fn open_checkpoints_the_migrations_out_of_the_wal() {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw.duckdb");
        {
            let _db = Db::open_without_checkpoint(&raw).unwrap();
            assert!(wal_len(&raw) > 0, "migrations alone leave the log populated");
        }
        let clean = dir.path().join("clean.duckdb");
        {
            let db = Db::open(&clean).unwrap();
            assert_eq!(wal_len(&clean), 0, "open folded the migrations into the file");
            db.with_conn(|c| { c.execute("insert into projects (id, name, root_path) values (?, ?, ?)", duckdb::params![uuid::Uuid::new_v4().to_string(), "p", "/tmp/p"])?; Ok(()) }).unwrap();
            assert!(wal_len(&clean) > 0, "a write after open lands in the log");
            db.checkpoint().unwrap();
            assert_eq!(wal_len(&clean), 0, "checkpoint folds it in");
        }
        let again = Db::open(&clean).unwrap();
        let n: i64 = again.with_conn(|c| Ok(c.query_row("select count(*) from projects", [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 1);
    }
    #[test]
    fn migrate_creates_tables_and_is_idempotent() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), 15);
        let n: i64 = db.with_conn(|c| Ok(c.query_row(
            "select count(*) from information_schema.tables where table_name in ('memories','memory_embeddings','audit','settings','projects','practices','workflow_docs','sync_targets','jobs','tasks','task_blockers','task_events','board_counters','workflows','workflow_runs','workflow_steps','skills','personas','project_personas')",
            [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 19);
        // Migrations 3, 5, 8 and 9 widen `projects` in place.
        let cols: i64 = db.with_conn(|c| Ok(c.query_row(
            "select count(*) from information_schema.columns where table_name='projects' and column_name in ('board_key','board_stages','agent_access','extraction','mcp_disabled_tools','skills_disabled')",
            [], |r| r.get(0))?)).unwrap();
        assert_eq!(cols, 6);
        db.migrate().unwrap(); // second run is a no-op
        assert_eq!(db.schema_version().unwrap(), 15);
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
        assert_eq!(db.schema_version().unwrap(), 15);
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
        assert_eq!(db.schema_version().unwrap(), 15);
        let n: i64 = db
            .with_conn(|c| {
                Ok(c.query_row("select count(*) from information_schema.tables where table_name = 'board_counters'", [], |r| r.get(0))?)
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    /// Migration 8 adds its column with `if not exists`, so replaying it over a
    /// database that already has it is a no-op rather than a failure.
    #[test]
    fn migration_8_is_a_no_op_on_a_database_that_already_has_the_column() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn(|c| {
            c.execute_batch("delete from schema_version where version >= 8;")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.schema_version().unwrap(), 7);
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 15);
        let n: i64 = db
            .with_conn(|c| {
                Ok(c.query_row(
                    "select count(*) from information_schema.columns where table_name = 'projects' and column_name = 'mcp_disabled_tools'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    /// Migration 9 creates its table and adds its column with `if not exists`, so
    /// replaying it over a database that already has both is a no-op rather than a
    /// failure.
    #[test]
    fn migration_9_is_a_no_op_on_a_database_that_already_has_the_table_and_column() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn(|c| {
            c.execute_batch("delete from schema_version where version >= 9;")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.schema_version().unwrap(), 8);
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 15);
        let n: i64 = db
            .with_conn(|c| {
                Ok(c.query_row(
                    "select count(*) from information_schema.columns where table_name = 'projects' and column_name = 'skills_disabled'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    fn index_count(db: &Db, table: &str, index: &str) -> i64 {
        db.with_conn(|c| {
            Ok(c.query_row("select count(*) from duckdb_indexes() where table_name = ? and index_name = ?", [table, index], |r| r.get(0))?)
        })
        .unwrap()
    }

    fn jobs_status_index_count(db: &Db) -> i64 {
        index_count(db, "jobs", "jobs_status_idx")
    }

    /// PERF-6 (ATL-308): migration 12 indexes `task_events(task_id)`, the column every
    /// task detail read and the task-event part of global search filter on. Same
    /// `if not exists` shape as migration 10: a fresh database and a database already
    /// at version 11 both end up with exactly one.
    #[test]
    fn migration_12_adds_the_task_events_task_id_index_to_a_v11_database() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(index_count(&db, "task_events", "task_events_task_id_idx"), 1, "a fresh database carries the index");
        db.with_conn(|c| {
            c.execute_batch("drop index task_events_task_id_idx; delete from schema_version where version >= 12;")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.schema_version().unwrap(), 11);
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 15);
        assert_eq!(index_count(&db, "task_events", "task_events_task_id_idx"), 1);
        db.migrate().unwrap();
        assert_eq!(index_count(&db, "task_events", "task_events_task_id_idx"), 1, "a replay is a no-op");
    }

    /// ATL-365: migration 13 rebuilds `tasks` without the kind check so `epic` and
    /// `feature_request` can be stored. A v12 database with the old check and a row in
    /// it comes through with the row intact and the new kinds accepted.
    #[test]
    fn migration_13_rebuilds_tasks_without_the_kind_check_on_a_v12_database() {
        let db = Db::open_in_memory().unwrap();
        db.with_conn(|c| {
            c.execute_batch(
                "insert into tasks (id, key, project_id, seq, title, stage, kind, priority, created_by) \
                 values ('11111111-1111-1111-1111-111111111111', 'ATL-1', null, 1, 'first', 'Backlog', 'epic', 'low', 'test');",
            )?;
            Ok(())
        })
        .expect("a fresh database accepts the new kinds");
        // Back to the v12 shape: the old table with the old check, holding one row.
        db.with_conn(|c| {
            c.execute_batch(
                "create table tasks_v12 (id uuid primary key, key text not null unique, project_id uuid, seq bigint not null, \
                 title text not null, description text not null default '', stage text not null, \
                 kind text not null check (kind in ('task','bug','feature','chore')), \
                 priority text not null check (priority in ('low','medium','high','urgent')), assignee text, \
                 labels json not null default '[]', parent_id uuid, created_by text not null, \
                 created_at timestamptz not null default now(), updated_at timestamptz not null default now(), \
                 closed_at timestamptz, source_ref json, persona_id uuid); \
                 insert into tasks_v12 select id, key, project_id, seq, title, description, stage, 'task', priority, assignee, labels, parent_id, created_by, created_at, updated_at, closed_at, source_ref, persona_id from tasks; \
                 drop table tasks; alter table tasks_v12 rename to tasks; \
                 delete from schema_version where version >= 13;",
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.schema_version().unwrap(), 12);
        let refused = db.with_conn(|c| {
            c.execute_batch("insert into tasks (id, key, seq, title, stage, kind, priority, created_by) values ('22222222-2222-2222-2222-222222222222', 'ATL-2', 2, 'x', 'Backlog', 'epic', 'low', 'test');")?;
            Ok(())
        });
        assert!(refused.is_err(), "the v12 check refuses epic");
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 15);
        let (count, title): (i64, String) = db
            .with_conn(|c| Ok(c.query_row("select count(*), min(title) from tasks", [], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .unwrap();
        assert_eq!((count, title.as_str()), (1, "first"), "the row survives the rebuild");
        db.with_conn(|c| {
            c.execute_batch("insert into tasks (id, key, seq, title, stage, kind, priority, created_by) values ('22222222-2222-2222-2222-222222222222', 'ATL-2', 2, 'x', 'Backlog', 'feature_request', 'low', 'test');")?;
            Ok(())
        })
        .expect("the rebuilt table accepts feature_request");
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 15, "a replay is a no-op");
    }

    /// Migration 10 adds the `jobs(status)` index with `if not exists`: a fresh
    /// database gets it, and a database already at version 9 (the previous release)
    /// gets it too, without failing when the index is already there.
    #[test]
    fn migration_10_adds_the_jobs_status_index_to_a_v9_database() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(jobs_status_index_count(&db), 1, "a fresh database carries the index");
        db.with_conn(|c| {
            c.execute_batch("drop index jobs_status_idx; delete from schema_version where version >= 10;")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.schema_version().unwrap(), 9);
        assert_eq!(jobs_status_index_count(&db), 0);
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 15);
        assert_eq!(jobs_status_index_count(&db), 1);

        // Replaying over a database that already has the index is a no-op.
        db.with_conn(|c| {
            c.execute_batch("delete from schema_version where version >= 10;")?;
            Ok(())
        })
        .unwrap();
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 15);
        assert_eq!(jobs_status_index_count(&db), 1);
    }

    fn persona_object_count(db: &Db) -> i64 {
        db.with_conn(|c| Ok(c.query_row(
            "select (select count(*) from information_schema.tables where table_name in ('personas', 'project_personas')) \
             + (select count(*) from information_schema.columns where table_name = 'tasks' and column_name = 'persona_id')",
            [], |r| r.get(0))?)).unwrap()
    }

    /// Migration 11 adds the persona tables and `tasks.persona_id` with `if not
    /// exists`: a fresh database gets them, a database at version 10 (the previous
    /// release) gets them too, and a replay over one that already has them is a no-op.
    #[test]
    fn migration_11_adds_the_persona_tables_to_a_v10_database() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(persona_object_count(&db), 3, "a fresh database carries both tables and the column");
        db.with_conn(|c| {
            c.execute_batch(
                "drop table project_personas; drop table personas; alter table tasks drop column persona_id; \
                 delete from schema_version where version >= 11;",
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(db.schema_version().unwrap(), 10);
        assert_eq!(persona_object_count(&db), 0);
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 15);
        assert_eq!(persona_object_count(&db), 3);

        db.with_conn(|c| {
            c.execute_batch("delete from schema_version where version >= 11;")?;
            Ok(())
        })
        .unwrap();
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 15);
        assert_eq!(persona_object_count(&db), 3);
    }

    #[test]
    fn open_on_disk_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.duckdb");
        let _db = Db::open(&p).unwrap();
        assert!(p.exists());
    }
}
