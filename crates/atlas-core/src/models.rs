use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! str_enum {
    ($name:ident { $($var:ident => $s:literal),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(rename_all = "lowercase")]
        pub enum $name { $($var),* }
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
