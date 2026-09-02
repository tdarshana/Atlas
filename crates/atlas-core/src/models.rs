use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

macro_rules! str_enum {
    ($name:ident { $($var:ident => $s:literal),* $(,)? }) => {
        // Each variant is renamed to the same literal `as_str`/`FromStr` use, so the JSON
        // wire form and the string form never drift apart (`rename_all = "lowercase"`
        // would spell `AgentsMd` as `agentsmd` while `as_str` says `agents_md`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
        pub enum $name { $(#[serde(rename = $s)] $var),* }
        impl $name {
            pub fn as_str(&self) -> &'static str { match self { $(Self::$var => $s),* } }
        }
        impl std::str::FromStr for $name {
            type Err = crate::AtlasError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s { $($s => Ok(Self::$var),)* _ => Err(crate::AtlasError::Invalid(format!("unknown {}: {s}", stringify!($name)))) }
            }
        }
        impl std::fmt::Display for $name { fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str(self.as_str()) } }
    };
}

str_enum!(MemoryScope { Global => "global", Project => "project" });
str_enum!(MemoryKind { Fact => "fact", Decision => "decision", Preference => "preference", Insight => "insight", Todo => "todo" });
str_enum!(MemoryStatus { Active => "active", Pending => "pending", Rejected => "rejected", Superseded => "superseded" });

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewMemory {
    pub scope: MemoryScope,
    pub project_id: Option<Uuid>,
    pub kind: MemoryKind,
    pub text: String,
    #[serde(default)] pub tags: Vec<String>,
    pub source_agent: Option<String>,
    pub source_tool: Option<String>,
    #[serde(default = "one")] pub confidence: f64,
    #[serde(default = "active")] pub status: MemoryStatus,
}
fn one() -> f64 { 1.0 }
fn active() -> MemoryStatus { MemoryStatus::Active }

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Memory {
    pub id: Uuid,
    pub scope: MemoryScope,
    pub project_id: Option<Uuid>,
    pub kind: MemoryKind,
    pub text: String,
    pub tags: Vec<String>,
    pub source_agent: Option<String>,
    pub source_tool: Option<String>,
    pub confidence: f64,
    pub status: MemoryStatus,
    pub superseded_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RecallHit { pub memory: Memory, pub score: f64 }

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StatusReport {
    pub version: String,
    pub db_path: String,
    pub memories_active: i64,
    pub embedding: String,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct ProjectProfile {
    pub name: String,
    #[serde(default)] pub languages: Vec<String>,
    #[serde(default)] pub frameworks: Vec<String>,
    #[serde(default)] pub tree: Vec<String>,
    #[serde(default)] pub readme_head: String,
    #[serde(default)] pub recent_commits: Vec<String>,
    #[serde(default)] pub summary: Option<String>,
    #[serde(default = "chrono::Utc::now")] pub built_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub root_path: String,
    pub git_remote: Option<String>,
    pub profile: Option<ProjectProfile>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewAgent {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub model_hint: Option<String>,
    #[serde(default)] pub tools: Vec<String>,
    #[serde(default)] pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Agent {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub model_hint: Option<String>,
    pub tools: Vec<String>,
    pub tags: Vec<String>,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

str_enum!(DocKind { Practice => "practice", Workflow => "workflow" });

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NewDoc {
    pub name: String,
    pub body: String,
    #[serde(default)] pub tags: Vec<String>,
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Doc {
    pub id: Uuid,
    pub kind: DocKind,
    pub name: String,
    pub body: String,
    pub tags: Vec<String>,
    pub project_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RecallQuery {
    pub query: String,
    #[serde(default = "ten")] pub limit: usize,
    #[serde(default)] pub scope: Option<MemoryScope>,
    #[serde(default)] pub project_id: Option<Uuid>,
    #[serde(default)] pub kinds: Vec<MemoryKind>,
    #[serde(default)] pub tags: Vec<String>,
}
fn ten() -> usize { 10 }

str_enum!(SyncKind { Claude => "claude", Codex => "codex", AgentsMd => "agents_md", ClaudeMd => "claude_md" });

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum SyncAction {
    Create,
    Update,
    Unchanged,
    Skip(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SyncOp {
    pub kind: SyncKind,
    pub path: PathBuf,
    pub content: String,
    pub action: SyncAction,
}

/// Everything an agent needs to start work in a project: the project itself,
/// the memories worth reading first, and the practices and workflows in scope.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProjectContext {
    pub project: Project,
    pub memories: Vec<RecallHit>,
    pub practices: Vec<Doc>,
    pub workflows: Vec<Doc>,
}

/// One sync request. `root` is required unless `global` is set, in which case
/// the sync targets a home directory instead of a project: `ATLAS_SYNC_HOME`
/// when set on the daemon process, else the daemon user's own home. There is
/// no client-side override; `POST /sync` is unauthenticated, so the request
/// itself must not be able to name an arbitrary write target.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct SyncRequest {
    #[serde(default)] pub root: Option<PathBuf>,
    #[serde(default)] pub global: bool,
    #[serde(default)] pub targets: Vec<SyncKind>,
    #[serde(default)] pub check_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, Default)]
pub struct SyncReport {
    pub ops: Vec<SyncOp>,
    pub created: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub skipped: usize,
}
