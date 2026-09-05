//! Generates `src/lib/api.generated.ts` from [`crate::routes::ROUTES`] and checks the
//! checked-in file against a fresh run. Test-only, like `atlas_core::tsgen`.
//!
//! Regenerate with
//!
//! ```text
//! ATLAS_WRITE_TS=1 cargo test -p atlasd --bin atlasd apigen
//! ```
//!
//! One method per `Client::Generated` entry, on an abstract class whose transport
//! (`req` and `text`) `src/lib/api.ts`'s `AtlasApi` supplies. The type names in the table
//! are the ones `src/lib/types.ts` exports, generated or hand-written; a name neither
//! file declares fails `every_type_the_table_names_is_declared` here and `bun run check`
//! in the desktop.

use crate::routes::{Client, Kind, Param, Route, ROUTES};
use std::fmt::Write;

const GENERATED_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/lib/api.generated.ts");
const TYPES_GENERATED_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/lib/types.generated.ts");
const TYPES_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/lib/types.ts");
const REGEN: &str = "ATLAS_WRITE_TS=1 cargo test -p atlasd --bin atlasd apigen";

/// TypeScript names that are not types of ours.
const BUILTINS: &[&str] = &["Record", "Promise", "Array", "Partial"];

pub fn generate() -> String {
    let mut ts = String::new();
    ts.push_str("// Generated from crates/atlasd/src/routes.rs by crates/atlasd/src/apigen.rs. Do not edit.\n");
    ts.push_str(&format!("// Regenerate with: {REGEN}\n"));
    ts.push_str("//\n// One method per route, on top of the transport `./api`'s `AtlasApi` supplies. The\n");
    ts.push_str("// routes the table marks hand-written stay in `./api`.\n\n");

    let mut names: Vec<String> = Vec::new();
    for r in ROUTES.iter().filter(|r| r.client == Client::Generated) {
        for t in type_names(r) {
            if !names.contains(&t) {
                names.push(t);
            }
        }
    }
    names.sort();
    ts.push_str("import type {\n");
    for n in &names {
        let _ = writeln!(ts, "\t{n},");
    }
    ts.push_str("} from './types';\n\n");

    ts.push_str("/** The `X-Atlas-Actor` the daemon records for every write the app makes; it is\n * ignored on a read. */\n");
    ts.push_str("const ACTOR = { 'X-Atlas-Actor': 'desktop' };\n\n");
    ts.push_str("/**\n * The query string for `params`: a null, undefined or empty value is left out, a\n * list is comma-joined, and anything else is stringified.\n */\n");
    ts.push_str("export function query(params: Record<string, unknown>): string {\n");
    ts.push_str("\tconst q = new URLSearchParams();\n");
    ts.push_str("\tfor (const [k, v] of Object.entries(params)) {\n");
    ts.push_str("\t\tif (v == null) continue;\n");
    ts.push_str("\t\tconst s = Array.isArray(v) ? v.join(',') : String(v);\n");
    ts.push_str("\t\tif (s !== '') q.set(k, s);\n");
    ts.push_str("\t}\n");
    ts.push_str("\tconst s = q.toString();\n");
    ts.push_str("\treturn s ? `?${s}` : '';\n");
    ts.push_str("}\n\n");

    ts.push_str("export abstract class GeneratedApi {\n");
    ts.push_str("\t/** The transport: JSON in and out, a non-2xx thrown as `ApiError`. */\n");
    ts.push_str("\tprotected abstract req<T>(method: string, path: string, body?: unknown, extra?: Record<string, string>): Promise<T>;\n");
    ts.push_str("\t/** The same, for a route whose answer is not JSON. */\n");
    ts.push_str("\tprotected abstract text(method: string, path: string): Promise<string>;\n");
    for r in ROUTES.iter().filter(|r| r.client == Client::Generated) {
        method(&mut ts, r);
    }
    ts.push_str("}\n");
    ts
}

fn method(ts: &mut String, r: &Route) {
    ts.push('\n');
    if !r.doc.is_empty() {
        let lines: Vec<&str> = r.doc.lines().collect();
        if lines.len() == 1 {
            let _ = writeln!(ts, "\t/** {} */", lines[0]);
        } else {
            ts.push_str("\t/**\n");
            for l in lines {
                let _ = writeln!(ts, "\t * {l}");
            }
            ts.push_str("\t */\n");
        }
    }
    let params: Vec<String> = r.params.iter().map(signature).collect();
    let response = match r.response {
        "text" => "string".to_string(),
        other => other.to_string(),
    };
    let _ = writeln!(ts, "\t{}({}): Promise<{response}> {{", r.name, params.join(", "));

    let payload = match r.body {
        None => None,
        Some(body) => {
            if let Some(whole) = r.params.iter().find(|p| p.kind == Kind::Body) {
                Some(whole.arg.to_string())
            } else {
                let fields: Vec<String> = r
                    .params
                    .iter()
                    .filter_map(|p| match p.kind {
                        Kind::Field(key) if key == p.arg => Some(key.to_string()),
                        Kind::Field(key) => Some(format!("{key}: {}", p.arg)),
                        _ => None,
                    })
                    .collect();
                let _ = writeln!(ts, "\t\tconst payload: {body} = {{ {} }};", fields.join(", "));
                Some("payload".to_string())
            }
        }
    };

    let path = path_expr(r);
    if r.response == "text" {
        let _ = writeln!(ts, "\t\treturn this.text('{}', {path});", r.method);
    } else {
        let mut args = format!("'{}', {path}", r.method);
        match (&payload, r.actor) {
            (Some(p), true) => { let _ = write!(args, ", {p}, ACTOR"); }
            (Some(p), false) => { let _ = write!(args, ", {p}"); }
            (None, true) => args.push_str(", undefined, ACTOR"),
            (None, false) => {}
        }
        let _ = writeln!(ts, "\t\treturn this.req({args});");
    }
    ts.push_str("\t}\n");
}

fn signature(p: &Param) -> String {
    match (p.kind, p.optional) {
        (Kind::QueryObject(_), true) => format!("{}: {} = {{}}", p.arg, p.ty),
        (_, true) => format!("{}?: {}", p.arg, p.ty),
        (_, false) => format!("{}: {}", p.arg, p.ty),
    }
}

/// The path as a TypeScript expression: a quoted string when it is fixed, a template
/// literal when a placeholder or a query parameter goes in.
fn path_expr(r: &Route) -> String {
    let mut path_params = r.params.iter().filter(|p| p.kind == Kind::Path);
    let mut out = String::new();
    for (i, seg) in r.path.split('/').enumerate() {
        if i > 0 {
            out.push('/');
        }
        if seg.starts_with('{') {
            let p = path_params.next().unwrap_or_else(|| panic!("{}: {} has more placeholders than Path params", r.name, r.path));
            let _ = write!(out, "${{encodeURIComponent({})}}", p.arg);
        } else {
            out.push_str(seg);
        }
    }
    assert!(path_params.next().is_none(), "{}: more Path params than placeholders in {}", r.name, r.path);

    let mut pairs: Vec<String> = Vec::new();
    for p in r.params {
        match p.kind {
            Kind::Query(key) if key == p.arg => pairs.push(key.to_string()),
            Kind::Query(key) => pairs.push(format!("{key}: {}", p.arg)),
            Kind::QueryObject(fields) => {
                for (key, field) in fields {
                    pairs.push(format!("{key}: {}.{field}", p.arg));
                }
            }
            _ => {}
        }
    }
    if !pairs.is_empty() {
        let _ = write!(out, "${{query({{ {} }})}}", pairs.join(", "));
    }
    if out.contains("${") { format!("`{out}`") } else { format!("'{out}'") }
}

/// The type names one route mentions: in its response, its body and its parameters.
fn type_names(r: &Route) -> Vec<String> {
    let mut sources: Vec<&str> = vec![r.response];
    sources.extend(r.body);
    sources.extend(r.params.iter().map(|p| p.ty));
    let mut names = Vec::new();
    for src in sources {
        for word in src.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !BUILTINS.contains(&word) && !names.iter().any(|n| n == word) {
                names.push(word.to_string());
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_names_are_unique_and_paths_match_their_params() {
        let mut seen = std::collections::HashSet::new();
        for r in ROUTES.iter().filter(|r| r.client != Client::None) {
            assert!(seen.insert(r.name), "route name listed twice: {}", r.name);
            assert!(r.path.starts_with("/api/v1/"), "{}: not an /api/v1 path", r.name);
            let placeholders = r.path.split('/').filter(|s| s.starts_with('{')).count();
            let path_params = r.params.iter().filter(|p| p.kind == Kind::Path).count();
            assert_eq!(placeholders, path_params, "{}: {} placeholders, {} Path params", r.name, placeholders, path_params);
            let fields = r.params.iter().filter(|p| matches!(p.kind, Kind::Field(_))).count();
            let whole = r.params.iter().filter(|p| p.kind == Kind::Body).count();
            assert!(whole <= 1 && (whole == 0 || fields == 0), "{}: a body is whole or by field, not both", r.name);
            assert_eq!(r.body.is_some(), whole + fields > 0, "{}: body and params disagree", r.name);
            let mut optional_seen = false;
            for p in r.params {
                assert!(!optional_seen || p.optional, "{}: required parameter {} after an optional one", r.name, p.arg);
                optional_seen |= p.optional;
            }
        }
    }

    /// Every type name the table uses is exported by `types.generated.ts` or `types.ts`,
    /// so a renamed model fails here rather than in the desktop's type check.
    #[test]
    fn every_type_the_table_names_is_declared() {
        let declared = format!(
            "{}\n{}",
            std::fs::read_to_string(TYPES_GENERATED_PATH).expect("read types.generated.ts"),
            std::fs::read_to_string(TYPES_PATH).expect("read types.ts")
        );
        let is_declared = |name: &str| {
            ["export interface ", "export type ", "export class "].iter().any(|prefix| {
                declared.lines().any(|l| l.strip_prefix(prefix).is_some_and(|rest| rest.starts_with(name) && !rest[name.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')))
            })
        };
        let mut missing = Vec::new();
        for r in ROUTES.iter().filter(|r| r.client != Client::None) {
            for t in type_names(r) {
                if !is_declared(&t) {
                    missing.push(format!("{} ({})", t, r.name));
                }
            }
        }
        assert!(missing.is_empty(), "types named in routes.rs that src/lib/types.ts does not export: {missing:?}");
    }

    #[test]
    fn generated_api_matches_routes() {
        let fresh = generate();
        if std::env::var_os("ATLAS_WRITE_TS").is_some() {
            std::fs::write(GENERATED_PATH, &fresh).expect("write api.generated.ts");
            return;
        }
        let checked_in = std::fs::read_to_string(GENERATED_PATH).unwrap_or_default();
        assert!(
            checked_in == fresh,
            "src/lib/api.generated.ts is out of date with crates/atlasd/src/routes.rs. Regenerate with:\n    {REGEN}"
        );
    }
}
