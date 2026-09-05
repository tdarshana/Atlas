//! Renders a roster persona for the agents' own files: a Claude Code subagent file
//! under `.claude/agents/<slug>.md`, a `## Persona: <name>` section for the managed
//! block in `AGENTS.md` (Codex has no per-agent file that carries a body), and the
//! `## Personas` roster section for the managed block in `CLAUDE.md`.

use super::claude::yaml_scalar;
use super::managed_block::neutralize;
use super::GENERATED_HEADER;
use crate::models::{Case, PersonaBundle};
use uuid::Uuid;

/// The second comment line of an exported persona file, followed by the slug. It is
/// what tells a persona export apart from an agent export: `sync` may delete the
/// former when the persona leaves the roster, and must never touch the latter.
pub const PERSONA_MARKER: &str = "atlas persona:";

/// True when the first few lines carry [`GENERATED_HEADER`] and [`PERSONA_MARKER`],
/// meaning the file is a persona export this exporter owns outright.
pub fn is_persona_export(content: &str) -> bool {
    super::is_generated(content) && content.lines().take(4).any(|l| l.contains(PERSONA_MARKER))
}

/// The `tools:` value: the persona's Atlas tools as `mcp__atlas__<tool>` and each
/// allowed server as `mcp__<name>__*`, the server-wide form the installed Claude Code
/// agent files under `~/.claude/agents` already use. A persona whose `tools` is empty
/// may see every Atlas tool, so when it names servers the Atlas server goes in
/// server-wide too; a `tools:` line that named only other servers would cut the
/// subagent off from Atlas. Empty when the persona names neither.
fn tools_value(b: &PersonaBundle) -> Vec<String> {
    let mut tools: Vec<String> = b.persona.tools.iter().map(|t| format!("mcp__atlas__{t}")).collect();
    if tools.is_empty() && !b.mcp_servers.is_empty() {
        tools.push("mcp__atlas__*".to_string());
    }
    tools.extend(b.mcp_servers.iter().map(|s| format!("mcp__{}__*", s.name)));
    tools
}

/// `<role>. <summary>`, dropping whichever half is empty.
fn description(b: &PersonaBundle) -> String {
    let role = b.persona.role.trim().trim_end_matches('.');
    let summary = b.persona.summary.trim();
    match (role.is_empty(), summary.is_empty()) {
        (true, _) => summary.to_string(),
        (false, true) => format!("{role}."),
        (false, false) => format!("{role}. {summary}"),
    }
}

/// The first line of a practice body, as its one-line summary.
fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("").trim()
}

/// The `Skills`, `Practices`, `Workflows` and `Models by case` sections, at heading
/// `level` (`##` in the subagent file, `###` under a `## Persona:` heading). An empty
/// list renders no section at all.
fn sections(b: &PersonaBundle, level: &str) -> String {
    let mut out = String::new();
    let mut section = |title: &str, items: Vec<String>| {
        if items.is_empty() {
            return;
        }
        out.push_str(&format!("\n{level} {title}\n\n"));
        for item in items {
            out.push_str(&format!("- {item}\n"));
        }
    };
    section("Skills", b.skills.iter().map(|s| format!("`{}`: {}", s.name, s.description.trim())).collect());
    section("Practices", b.practices.iter().map(|p| format!("`{}`: {}", p.name, first_line(&p.body))).collect());
    section(
        "Workflows",
        b.workflows
            .iter()
            .map(|w| {
                let actions = if w.action_count == 1 { "1 action".to_string() } else { format!("{} actions", w.action_count) };
                format!("`{}`: {} trigger, {actions}", w.name, w.trigger.as_str())
            })
            .collect(),
    );
    section("Models by case", b.persona.models.iter().map(|(case, model)| format!("`{}`: {model}", case.as_str())).collect());
    out
}

/// Renders a Claude Code subagent file for a persona: frontmatter with the slug as
/// `name`, the role and summary as `description`, the default-case model as `model`
/// and the tool allowances, then the instructions and the reference sections.
pub fn claude_subagent(b: &PersonaBundle) -> String {
    let p = &b.persona;
    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("# {GENERATED_HEADER}\n"));
    out.push_str(&format!("# {PERSONA_MARKER} {}\n", p.slug));
    out.push_str(&format!("name: {}\n", yaml_scalar(&p.slug)));
    out.push_str(&format!("description: {}\n", yaml_scalar(&description(b))));
    let tools = tools_value(b);
    if !tools.is_empty() {
        out.push_str(&format!("tools: {}\n", yaml_scalar(&tools.join(", "))));
    }
    if let Some(model) = p.models.get(&Case::Default) {
        out.push_str(&format!("model: {}\n", yaml_scalar(model)));
    }
    out.push_str("---\n\n");
    out.push_str(p.instructions.trim_end());
    out.push('\n');
    out.push_str(&sections(b, "##"));
    out
}

/// The same content as [`claude_subagent`], as one `## Persona: <name>` section for
/// the managed block Codex reads in `AGENTS.md`. Every interpolated value goes
/// through `neutralize` so a body cannot close the block early.
pub fn agents_md_section(b: &PersonaBundle) -> String {
    let p = &b.persona;
    let mut out = format!("## Persona: {}\n\n", neutralize(&p.name));
    let description = description(b);
    if !description.is_empty() {
        out.push_str(&neutralize(&description));
        out.push('\n');
    }
    if let Some(model) = p.models.get(&Case::Default) {
        out.push_str(&format!("\nModel: `{}`\n", neutralize(model)));
    }
    let tools = tools_value(b);
    if !tools.is_empty() {
        out.push_str(&format!("\nTools: {}\n", neutralize(&tools.iter().map(|t| format!("`{t}`")).collect::<Vec<_>>().join(", "))));
    }
    let instructions = p.instructions.trim();
    if !instructions.is_empty() {
        out.push('\n');
        out.push_str(&neutralize(instructions));
        out.push('\n');
    }
    out.push_str(&neutralize(&sections(b, "###")));
    out
}

/// Every roster persona as one `## Persona:` section after another, in roster order.
/// Empty for an empty roster, so a project without personas renders as before.
pub fn agents_md_sections(bundles: &[PersonaBundle]) -> String {
    bundles.iter().map(agents_md_section).collect::<Vec<_>>().join("\n")
}

/// The `## Personas` section for the managed block in `CLAUDE.md`: the roster with
/// roles, which one is the default, and the two ways a session adopts one. Empty for
/// an empty roster.
pub fn claude_md_personas(bundles: &[PersonaBundle], default: Option<Uuid>) -> String {
    if bundles.is_empty() {
        return String::new();
    }
    let mut out = String::from("## Personas\n\n");
    out.push_str("This project's roster, in order. A persona is adopted for a session by claiming a task that names one (`task_claim`) or by calling `persona_use` with its slug; `persona_use` with an empty name clears it.\n\n");
    for b in bundles {
        let p = &b.persona;
        let mut line = format!("- `{}` ({})", neutralize(&p.slug), neutralize(&p.name));
        let role = p.role.trim();
        if !role.is_empty() {
            line.push_str(&format!(": {}", neutralize(role)));
        }
        if default == Some(p.id) {
            line.push_str(" (default)");
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::*;
    use std::collections::BTreeMap;

    fn persona(name: &str, slug: &str) -> Persona {
        Persona {
            id: Uuid::nil(),
            name: name.into(),
            slug: slug.into(),
            role: "Builds the app".into(),
            summary: "Owns the mobile client end to end.".into(),
            instructions: "Prefer small commits.\n\nAsk before adding a dependency.".into(),
            skills: vec![],
            workflows: vec![],
            practices: vec![],
            mcp_servers: vec![],
            tools: vec![],
            access: PersonaAccess::default(),
            models: BTreeMap::new(),
            tags: vec![],
            created_at: Default::default(),
            updated_at: Default::default(),
        }
    }

    fn server(name: &str) -> McpServerEntry {
        McpServerEntry {
            id: format!("claude:user:{name}"),
            name: name.into(),
            source: McpServerSource::Claude,
            scope: McpServerScope::User,
            transport: McpTransport::Stdio { command: "npx".into(), args: vec![], env_keys: vec![] },
            file: None,
            plugin: None,
            enabled: true,
            can_toggle: true,
            can_remove: true,
            is_atlas: false,
            project_id: None,
        }
    }

    fn full_bundle() -> PersonaBundle {
        let mut p = persona("Mobile Developer", "mobile-developer");
        p.tools = vec!["task_list".into(), "memory_search".into()];
        p.models = BTreeMap::from([(Case::Default, "sonnet".to_string()), (Case::Review, "opus".to_string())]);
        PersonaBundle {
            persona: p,
            skills: vec![SkillSummary {
                id: "plugin:sm/rn/best-practices".into(),
                source: SkillSource::Plugin,
                name: "react-native-best-practices".into(),
                description: "Production patterns for the New Architecture.".into(),
                scope: MemoryScope::Global,
                project_id: None,
                path: None,
                plugin: Some("sm/rn".into()),
                editable: false,
                updated_at: None,
                enabled_here: None,
            }],
            workflows: vec![WorkflowSummary { id: Uuid::nil(), name: "nightly".into(), trigger: TriggerKind::Schedule, action_count: 2, enabled: true, last_status: None }],
            practices: vec![Doc {
                id: Uuid::nil(),
                kind: DocKind::Practice,
                name: "commits".into(),
                body: "Imperative mood, one change per commit.\n\nMore detail here.".into(),
                tags: vec![],
                project_id: None,
                created_at: Default::default(),
                updated_at: Default::default(),
            }],
            mcp_servers: vec![server("context7")],
            warnings: vec![],
        }
    }

    #[test]
    fn a_full_bundle_renders_the_frontmatter_and_every_section() {
        let expected = "---\n\
# generated by atlas; edit in Atlas, not here\n\
# atlas persona: mobile-developer\n\
name: mobile-developer\n\
description: Builds the app. Owns the mobile client end to end.\n\
tools: mcp__atlas__task_list, mcp__atlas__memory_search, mcp__context7__*\n\
model: sonnet\n\
---\n\
\n\
Prefer small commits.\n\
\n\
Ask before adding a dependency.\n\
\n\
## Skills\n\
\n\
- `react-native-best-practices`: Production patterns for the New Architecture.\n\
\n\
## Practices\n\
\n\
- `commits`: Imperative mood, one change per commit.\n\
\n\
## Workflows\n\
\n\
- `nightly`: schedule trigger, 2 actions\n\
\n\
## Models by case\n\
\n\
- `review`: opus\n\
- `default`: sonnet\n";
        assert_eq!(claude_subagent(&full_bundle()), expected);
        assert!(is_persona_export(&claude_subagent(&full_bundle())));
        assert!(!is_persona_export("---\n# generated by atlas; edit in Atlas, not here\nname: reviewer\n---\n"), "an agent export is not a persona export");
    }

    #[test]
    fn an_empty_tools_list_and_no_servers_render_no_tools_line() {
        let bundle = PersonaBundle { persona: persona("Ops", "ops"), skills: vec![], workflows: vec![], practices: vec![], mcp_servers: vec![], warnings: vec![] };
        let md = claude_subagent(&bundle);
        assert!(!md.contains("tools:"), "{md}");
        assert_eq!(md, "---\n# generated by atlas; edit in Atlas, not here\n# atlas persona: ops\nname: ops\ndescription: Builds the app. Owns the mobile client end to end.\n---\n\nPrefer small commits.\n\nAsk before adding a dependency.\n");
    }

    /// A persona that may use every Atlas tool but names another server still gets
    /// Atlas server-wide: a `tools:` line is a whitelist.
    #[test]
    fn servers_without_atlas_tools_keep_atlas_server_wide() {
        let mut bundle = full_bundle();
        bundle.persona.tools.clear();
        let md = claude_subagent(&bundle);
        assert!(md.contains("tools: mcp__atlas__*, mcp__context7__*\n"), "{md}");
    }

    #[test]
    fn a_missing_default_model_renders_no_model_line() {
        let mut bundle = full_bundle();
        bundle.persona.models.remove(&Case::Default);
        let md = claude_subagent(&bundle);
        assert!(!md.contains("\nmodel:"), "{md}");
        assert!(md.contains("- `review`: opus\n") && !md.contains("`default`"), "{md}");
    }

    #[test]
    fn the_agents_md_section_carries_the_same_content_under_one_heading() {
        let section = agents_md_section(&full_bundle());
        let expected = "## Persona: Mobile Developer\n\
\n\
Builds the app. Owns the mobile client end to end.\n\
\n\
Model: `sonnet`\n\
\n\
Tools: `mcp__atlas__task_list`, `mcp__atlas__memory_search`, `mcp__context7__*`\n\
\n\
Prefer small commits.\n\
\n\
Ask before adding a dependency.\n\
\n\
### Skills\n\
\n\
- `react-native-best-practices`: Production patterns for the New Architecture.\n\
\n\
### Practices\n\
\n\
- `commits`: Imperative mood, one change per commit.\n\
\n\
### Workflows\n\
\n\
- `nightly`: schedule trigger, 2 actions\n\
\n\
### Models by case\n\
\n\
- `review`: opus\n\
- `default`: sonnet\n";
        assert_eq!(section, expected);
    }

    #[test]
    fn a_marker_in_the_instructions_cannot_close_the_block() {
        let mut bundle = full_bundle();
        bundle.persona.instructions = "Never paste <!-- atlas:end --> anywhere.".into();
        let section = agents_md_section(&bundle);
        assert!(!section.contains(super::super::END), "{section}");
    }

    #[test]
    fn the_claude_md_section_names_the_roster_the_default_and_both_ways_in() {
        let mut second = full_bundle();
        second.persona.id = Uuid::from_u128(2);
        second.persona.name = "Security Reviewer".into();
        second.persona.slug = "security-reviewer".into();
        second.persona.role = "Reviews for risk".into();
        let text = claude_md_personas(&[full_bundle(), second], Some(Uuid::from_u128(2)));
        let expected = "## Personas\n\
\n\
This project's roster, in order. A persona is adopted for a session by claiming a task that names one (`task_claim`) or by calling `persona_use` with its slug; `persona_use` with an empty name clears it.\n\
\n\
- `mobile-developer` (Mobile Developer): Builds the app\n\
- `security-reviewer` (Security Reviewer): Reviews for risk (default)\n";
        assert_eq!(text, expected);
        assert_eq!(claude_md_personas(&[], None), "");
        assert_eq!(agents_md_sections(&[]), "");
    }
}
