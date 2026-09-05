//! Generates `src/lib/types.generated.ts` from the `schemars::JsonSchema` derives on the
//! API models, and checks the checked-in file against a fresh run. Test-only: the desktop
//! app has no reason to carry a code generator at run time.
//!
//! Regenerate with
//!
//! ```text
//! ATLAS_WRITE_TS=1 cargo test -p atlas-core --lib tsgen
//! ```
//!
//! Each type is emitted once, under one of two serde contracts. A type the daemon
//! answers with is emitted from the serialize contract: every field is present, an
//! `Option` is `T | null`. A type the daemon reads is emitted from the deserialize
//! contract: a `#[serde(default)]` or `Option` field is `?`. The two lists below say
//! which is which; the coverage test fails when a `JsonSchema` type is in neither.

use schemars::generate::SchemaSettings;
use schemars::SchemaGenerator;
use serde_json::{Map, Value};
use std::fmt::Write;

use crate::models::*;
use crate::search::global::{SearchGroup, SearchHit, SearchKind, SearchQuery, SearchResult};

const GENERATED_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/lib/types.generated.ts");
const REGEN: &str = "ATLAS_WRITE_TS=1 cargo test -p atlas-core --lib tsgen";

/// The shapes the daemon answers with. Emitted from the serialize contract.
fn outputs(g: &mut SchemaGenerator) -> Vec<&'static str> {
    macro_rules! list { ($($t:ident),* $(,)?) => {{ $(g.subschema_for::<$t>();)* vec![$(stringify!($t)),*] }}; }
    list![
        MemoryScope, MemoryKind, MemoryStatus, MemoryScopeFilter, Memory, RecallHit, MemoryFacets,
        StatusReport,
        FrameworkKind, FrameworkInventory, FrameworkDocType, FrameworkDoc, SourceRef, FrameworkListing,
        ImportWhat, ImportReport, ImportedTask, ImportedDecision,
        ProjectProfile, Project, AgentAccess, ProjectAccess, LogRef, LogEntry, ProjectContext,
        Agent, DocKind, Doc,
        SyncKind, SyncAction, SyncOp, SyncReport,
        TaskKind, TaskPriority, Stage, StageList, Task, TaskEvent, TaskDetail, Change,
        TriggerKind, NodeKind, RunStatus, StepStatus, LogLevel, Trigger, Position, MemorySource,
        NodeData, Node, Edge, Graph, Workflow, WorkflowSummary, WorkflowRun, LogLine, WorkflowStep,
        SearchKind, SearchHit, SearchGroup, SearchResult,
        PluginToolScope, PluginToolDecl,
        McpServerSource, McpServerScope, McpTransport, McpServerEntry, McpServerList, McpToolInfo,
        McpCheckResult,
        SkillSource, SkillSummary, Skill, SkillList,
        Case, PersonaRule, PersonaAccess, Persona, RosterRow, PersonaBundle, PersonaContext,
    ]
}

/// The shapes the daemon reads. Emitted from the deserialize contract.
fn inputs(g: &mut SchemaGenerator) -> Vec<&'static str> {
    macro_rules! list { ($($t:ident),* $(,)?) => {{ $(g.subschema_for::<$t>();)* vec![$(stringify!($t)),*] }}; }
    list![
        NewMemory, RecallQuery, ProjectExtraction, ProjectPatch, LogFilter, NewAgent, NewDoc,
        SyncRequest, NewTask, TaskUpdate, TaskFilter, NewWorkflow, WorkflowPatch, SearchQuery,
        McpTransportInput, NewMcpServer, NewSkill, SkillUpdate,
        NewPersona, PersonaUpdate, RosterEntry,
    ]
}

pub fn generate() -> String {
    let mut ser = SchemaGenerator::new(SchemaSettings::draft2020_12().for_serialize());
    let mut de = SchemaGenerator::new(SchemaSettings::draft2020_12().for_deserialize());
    let out_names = outputs(&mut ser);
    let in_names = inputs(&mut de);
    let ser_defs = ser.take_definitions(true);
    let de_defs = de.take_definitions(true);

    let mut ts = String::new();
    ts.push_str("// Generated from the `schemars::JsonSchema` derives in crates/atlas-core/src/models.rs and\n");
    ts.push_str("// crates/atlas-core/src/search/global.rs by crates/atlas-core/src/tsgen.rs. Do not edit.\n");
    ts.push_str(&format!("// Regenerate with: {REGEN}\n"));
    ts.push_str("//\n// UUIDs and `DateTime<Utc>` both travel as strings; a `str_enum!` is its literal union.\n\n");
    ts.push_str("export type Uuid = string;\n/** RFC 3339, e.g. \"2026-09-02T10:30:00Z\". */\nexport type Timestamp = string;\n");

    ts.push_str("\n// ---- shapes the daemon answers with (every field present, Option is `T | null`) ----\n");
    for name in &out_names {
        emit_named(&mut ts, name, &ser_defs[*name]);
    }
    ts.push_str("\n// ---- shapes the daemon reads (a defaulted or Option field may be left out) ----\n");
    for name in &in_names {
        emit_named(&mut ts, name, &de_defs[*name]);
    }
    ts
}

fn emit_named(ts: &mut String, name: &str, schema: &Value) {
    ts.push('\n');
    if let Some(d) = schema.get("description").and_then(Value::as_str) {
        doc(ts, d, "");
    }
    match schema.as_object() {
        Some(o) if o.contains_key("properties") && !o.contains_key("anyOf") && !o.contains_key("oneOf") => {
            let _ = writeln!(ts, "export interface {name} {{");
            fields(ts, o, "\t");
            ts.push_str("}\n");
        }
        _ => {
            let _ = writeln!(ts, "export type {name} = {};", expr(schema, ""));
        }
    }
}

fn doc(ts: &mut String, text: &str, indent: &str) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() == 1 {
        let _ = writeln!(ts, "{indent}/** {} */", lines[0].trim());
    } else {
        let _ = writeln!(ts, "{indent}/**");
        for l in lines {
            let l = l.trim();
            if l.is_empty() {
                let _ = writeln!(ts, "{indent} *");
            } else {
                let _ = writeln!(ts, "{indent} * {l}");
            }
        }
        let _ = writeln!(ts, "{indent} */");
    }
}

fn fields(ts: &mut String, obj: &Map<String, Value>, indent: &str) {
    let required: Vec<&str> = obj.get("required").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let props = obj.get("properties").and_then(Value::as_object);
    for (key, schema) in props.into_iter().flatten() {
        if let Some(d) = schema.get("description").and_then(Value::as_str) {
            doc(ts, d, indent);
        }
        let opt = if required.contains(&key.as_str()) { "" } else { "?" };
        let _ = writeln!(ts, "{indent}{}{opt}: {};", prop_name(key), expr(schema, indent));
    }
}

fn prop_name(key: &str) -> String {
    let ident = key.chars().enumerate().all(|(i, c)| c == '_' || c == '$' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()));
    if ident { key.to_string() } else { format!("'{key}'") }
}

/// The TypeScript expression for one schema. `indent` is the indentation of the line
/// the expression sits on, for inline object literals.
fn expr(schema: &Value, indent: &str) -> String {
    let obj = match schema {
        Value::Bool(_) => return "unknown".into(),
        Value::Object(o) => o,
        other => panic!("unexpected schema {other}"),
    };
    if let Some(r) = obj.get("$ref").and_then(Value::as_str) {
        return r.rsplit('/').next().unwrap().to_string();
    }
    if let Some(members) = obj.get("anyOf").or_else(|| obj.get("oneOf")).and_then(Value::as_array) {
        if members.iter().any(|m| m.as_bool() == Some(true)) {
            return "unknown".into();
        }
        return members.iter().map(|m| expr(m, indent)).collect::<Vec<_>>().join(" | ");
    }
    if let Some(members) = obj.get("allOf").and_then(Value::as_array) {
        return members.iter().map(|m| expr(m, indent)).collect::<Vec<_>>().join(" & ");
    }
    if let Some(c) = obj.get("const") {
        return literal(c);
    }
    if let Some(values) = obj.get("enum").and_then(Value::as_array) {
        return values.iter().map(literal).collect::<Vec<_>>().join(" | ");
    }
    let types: Vec<&str> = match obj.get("type") {
        Some(Value::String(s)) => vec![s.as_str()],
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => return "unknown".into(),
    };
    let mut parts: Vec<String> = types.iter().map(|t| typed(t, obj, indent)).collect();
    // `T | null` reads better than `null | T`, whichever order the schema lists them in.
    parts.sort_by_key(|p| p.as_str() == "null");
    parts.join(" | ")
}

fn typed(t: &str, obj: &Map<String, Value>, indent: &str) -> String {
    match t {
        "null" => "null".into(),
        "boolean" => "boolean".into(),
        "integer" | "number" => "number".into(),
        "string" => match obj.get("format").and_then(Value::as_str) {
            Some("uuid") => "Uuid".into(),
            Some("date-time") => "Timestamp".into(),
            _ => "string".into(),
        },
        "array" => {
            if let Some(items) = obj.get("prefixItems").and_then(Value::as_array) {
                return format!("[{}]", items.iter().map(|i| expr(i, indent)).collect::<Vec<_>>().join(", "));
            }
            let item = obj.get("items").map(|i| expr(i, indent)).unwrap_or_else(|| "unknown".into());
            if item.contains('|') { format!("({item})[]") } else { format!("{item}[]") }
        }
        "object" => {
            if obj.contains_key("properties") {
                let inner = format!("{indent}\t");
                let mut body = String::new();
                fields(&mut body, obj, &inner);
                format!("{{\n{body}{indent}}}")
            } else if let Some(extra) = obj.get("additionalProperties") {
                format!("Record<string, {}>", expr(extra, indent))
            } else {
                "Record<string, unknown>".into()
            }
        }
        other => panic!("unhandled JSON Schema type {other}"),
    }
}

fn literal(v: &Value) -> String {
    match v {
        Value::String(s) => format!("'{}'", s.replace('\'', "\\'")),
        other => other.to_string(),
    }
}

/// Every `JsonSchema` type declared in the model sources, so a new model that is not in
/// `outputs` or `inputs` fails here rather than drifting silently.
fn declared_json_schema_types() -> Vec<String> {
    let sources = [include_str!("models.rs"), include_str!("search/global.rs")];
    let mut names = Vec::new();
    for src in sources {
        let mut derives_schema = false;
        for line in src.lines() {
            let line = line.trim_start();
            if let Some(rest) = line.strip_prefix("str_enum!(") {
                names.push(rest.split_whitespace().next().unwrap().to_string());
            } else if line.starts_with("#[derive(") {
                derives_schema = line.contains("JsonSchema");
            } else if let Some(rest) = line.strip_prefix("pub struct ").or_else(|| line.strip_prefix("pub enum ")) {
                let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$').collect();
                // `$name` is the `str_enum!` macro body, whose expansions are caught above.
                if derives_schema && !name.starts_with('$') {
                    names.push(name);
                }
                derives_schema = false;
            } else if !line.starts_with("#[") && !line.starts_with("///") {
                derives_schema = false;
            }
        }
    }
    names
}

#[test]
fn every_json_schema_model_is_listed() {
    let mut ser = SchemaGenerator::new(SchemaSettings::draft2020_12().for_serialize());
    let mut de = SchemaGenerator::new(SchemaSettings::draft2020_12().for_deserialize());
    let mut listed = outputs(&mut ser);
    listed.extend(inputs(&mut de));
    let missing: Vec<String> = declared_json_schema_types().into_iter().filter(|n| !listed.contains(&n.as_str())).collect();
    assert!(missing.is_empty(), "JsonSchema types not in tsgen::outputs or tsgen::inputs: {missing:?}");
    let mut seen = std::collections::HashSet::new();
    let dup: Vec<_> = listed.iter().filter(|n| !seen.insert(**n)).collect();
    assert!(dup.is_empty(), "types listed twice in tsgen: {dup:?}");
}

#[test]
fn generated_types_match_models() {
    let fresh = generate();
    if std::env::var_os("ATLAS_WRITE_TS").is_some() {
        std::fs::write(GENERATED_PATH, &fresh).expect("write types.generated.ts");
        return;
    }
    let checked_in = std::fs::read_to_string(GENERATED_PATH).unwrap_or_default();
    assert!(
        checked_in == fresh,
        "src/lib/types.generated.ts is out of date with the Rust models. Regenerate with:\n    {REGEN}"
    );
}
