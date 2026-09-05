//! A fake that implements one domain trait and is used through a function bound on
//! that trait alone. Before the `Backend` split a test double had to implement all
//! 76 methods; this one implements the three of `StatusBackend`.
use atlas_core::backend::StatusBackend;
use atlas_core::models::StatusReport;
use atlas_core::Result;
use atlas_mcp::disabled_tool_names;
use serde_json::{json, Map, Value};
use std::collections::HashSet;

struct SettingsOnly(Map<String, Value>);

#[async_trait::async_trait]
impl StatusBackend for SettingsOnly {
    async fn status(&self) -> Result<StatusReport> {
        unreachable!("disabled_tool_names never asks for the status")
    }
    async fn get_settings(&self) -> Result<Map<String, Value>> {
        Ok(self.0.clone())
    }
    async fn set_settings(&self, _values: Map<String, Value>, _actor: &str) -> Result<Map<String, Value>> {
        unreachable!("disabled_tool_names never writes")
    }
}

#[tokio::test]
async fn a_status_only_fake_serves_disabled_tool_names() {
    let mut settings = Map::new();
    settings.insert("mcp.disabled_tools".into(), json!(["memory_forget", "task_delete"]));
    let names = disabled_tool_names(&SettingsOnly(settings)).await.unwrap();
    let want: HashSet<String> = ["memory_forget", "task_delete"].into_iter().map(String::from).collect();
    assert_eq!(names, want);

    let default = disabled_tool_names(&SettingsOnly(Map::new())).await.unwrap();
    let want: HashSet<String> = atlas_core::settings::DEFAULT_DISABLED_MCP_TOOLS.iter().map(|s| s.to_string()).collect();
    assert_eq!(default, want);
}
