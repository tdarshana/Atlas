//! The daemon's `/api/v1` route table: one entry per route `http::router` mounts, as the
//! desktop's TypeScript client sees it. `apigen` writes `src/lib/api.generated.ts` from
//! it, and `tests/api.rs::every_route_in_the_table_is_served` asks a running daemon for
//! each entry, so a route dropped from either side fails a test.
//!
//! Plain data with no crate-internal dependency, so the integration test can include it
//! with `#[path]`; the type and field names are the TypeScript ones in `types.ts`.

/// Which client method serves a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    /// `apigen` writes the method from this entry.
    Generated,
    /// `src/lib/api.ts` keeps a hand-written method: the route needs something the
    /// table cannot say (a path chosen from an argument, a body that is not JSON, a
    /// per-call actor, a 400 read as data).
    HandWritten,
    /// No desktop method: a stream the browser opens itself (`EventSource`, `WebSocket`).
    None,
}

/// How one TypeScript parameter reaches the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Fills the next `{placeholder}` of `path`, percent-encoded.
    Path,
    /// One query parameter, under this wire key.
    Query(&'static str),
    /// An object whose fields become query parameters: `(wire key, field)` pairs.
    QueryObject(&'static [(&'static str, &'static str)]),
    /// The whole JSON body.
    Body,
    /// One field of the JSON body, under this key; the body type is `Route::body`.
    Field(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param {
    /// The TypeScript parameter name.
    pub arg: &'static str,
    /// Its TypeScript type.
    pub ty: &'static str,
    pub kind: Kind,
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    /// The TypeScript method name.
    pub name: &'static str,
    pub method: &'static str,
    /// The path as axum mounts it, `{id}`-style placeholders included.
    pub path: &'static str,
    /// The method's parameters, in signature order.
    pub params: &'static [Param],
    /// The TypeScript type of the JSON body, when the route reads one.
    pub body: Option<&'static str>,
    /// The TypeScript type of the answer; `void` for an empty one, `text` for a body
    /// that is not JSON.
    pub response: &'static str,
    /// Sends `X-Atlas-Actor: desktop`, so the daemon records the write against the app.
    pub actor: bool,
    pub client: Client,
    /// The method's JSDoc, empty for none.
    pub doc: &'static str,
}

const fn p(arg: &'static str, ty: &'static str, kind: Kind) -> Param { Param { arg, ty, kind, optional: false } }
const fn opt(arg: &'static str, ty: &'static str, kind: Kind) -> Param { Param { arg, ty, kind, optional: true } }

const PROJECT_Q: Param = opt("projectId", "Uuid | null", Kind::Query("project_id"));

macro_rules! route {
    ($name:literal, $method:literal, $path:literal, [$($param:expr),* $(,)?], body: $body:expr, response: $response:literal $(, actor: $actor:literal)? $(, client: $client:expr)? $(, doc: $doc:literal)?) => {
        Route {
            name: $name, method: $method, path: $path, params: &[$($param),*], body: $body, response: $response,
            actor: { #[allow(unused_variables)] let a = false; $(let a = $actor;)? a },
            client: { #[allow(unused_variables)] let c = Client::Generated; $(let c = $client;)? c },
            doc: { #[allow(unused_variables)] let d = ""; $(let d = $doc;)? d },
        }
    };
}

pub const ROUTES: &[Route] = &[
    route!("status", "GET", "/api/v1/status", [], body: None, response: "StatusReport"),
    route!("events", "GET", "/api/v1/events", [], body: None, response: "void", client: Client::None,
        doc: "The change stream; `stores/changes.svelte.ts` opens it as an `EventSource`."),

    // ---- memories ----
    route!("remember", "POST", "/api/v1/memories", [p("m", "NewMemory", Kind::Body)], body: Some("NewMemory"), response: "Memory"),
    route!("listMemories", "GET", "/api/v1/memories", [
        opt("status", "MemoryStatus", Kind::Query("status")),
        PROJECT_Q,
        opt("scope", "MemoryListScope", Kind::Query("scope")),
        opt("page", "MemoryPage", Kind::QueryObject(&[("limit", "limit"), ("offset", "offset")])),
    ], body: None, response: "Memory[]",
        doc: "`scope: 'project_only'` (paired with `projectId`) narrows to that project's own\nmemories, dropping the global ones a bare `project_id` still widens in. `page`\nwindows the newest-first list; without it the whole set comes back."),
    route!("memoryFacets", "GET", "/api/v1/memories/facets", [PROJECT_Q, opt("scope", "MemoryListScope | null", Kind::Query("scope"))], body: None, response: "MemoryFacets",
        doc: "Kind and tag counts, plus the total, over active memories, `project_id`/`scope`\nread the same way `listMemories` reads them. Lets a side panel show counts\nwithout loading every matching memory first."),
    route!("search", "POST", "/api/v1/memories/search", [p("q", "RecallQuery", Kind::Body)], body: Some("RecallQuery"), response: "RecallHit[]",
        doc: "`list_scope` is the same narrowing `listMemories` takes; `scope` stays the memory's\nown scope, and the daemon reads the two independently."),
    route!("getMemory", "GET", "/api/v1/memories/{id}", [p("id", "Uuid", Kind::Path)], body: None, response: "Memory"),
    route!("forget", "POST", "/api/v1/memories/{id}/forget", [p("id", "Uuid", Kind::Path), opt("reason", "string", Kind::Field("reason"))], body: Some("ForgetBody"), response: "Memory"),
    route!("setMemoryStatus", "POST", "/api/v1/memories/{id}/status", [p("id", "Uuid", Kind::Path), p("status", "MemoryStatus", Kind::Field("status"))], body: Some("StatusBody"), response: "Memory"),

    // ---- projects ----
    route!("listProjects", "GET", "/api/v1/projects", [], body: None, response: "Project[]"),
    route!("connectProject", "POST", "/api/v1/projects/connect", [p("root", "string", Kind::Field("root"))], body: Some("RootBody"), response: "Project"),
    route!("projectContext", "POST", "/api/v1/projects/context", [p("root", "string", Kind::Field("root"))], body: Some("RootBody"), response: "ProjectContext"),
    route!("getProject", "GET", "/api/v1/projects/{id}", [p("id", "Uuid", Kind::Path)], body: None, response: "Project"),
    route!("patchProject", "PATCH", "/api/v1/projects/{id}", [p("id", "Uuid", Kind::Path), p("patch", "ProjectPatch", Kind::Body)], body: Some("ProjectPatch"), response: "Project",
        doc: "Only the fields in `patch` change. `git_remote: null` clears the remote; leaving the\nfield out keeps it. A new `board_key` renames every task key in the project, so ask\nbefore sending one."),
    route!("deleteProject", "DELETE", "/api/v1/projects/{id}", [p("id", "Uuid", Kind::Path)], body: None, response: "void",
        doc: "Removes the project row. Its memories are kept; nothing is hard-deleted."),
    route!("refreshProject", "POST", "/api/v1/projects/{id}/refresh", [p("id", "Uuid", Kind::Path)], body: None, response: "Project"),
    route!("setAgentAccess", "PUT", "/api/v1/projects/{id}/agent-access", [p("id", "Uuid", Kind::Path), p("access", "AgentAccess", Kind::Body)], body: Some("AgentAccess"), response: "Project",
        doc: "Replaces the project's access rules wholesale; a null list means any actor."),
    route!("projectAccess", "GET", "/api/v1/projects/{id}/access", [p("id", "Uuid", Kind::Path)], body: None, response: "ProjectAccessReport",
        doc: "The project's rules, the global defaults and what they resolve to."),
    route!("setProjectExtraction", "PUT", "/api/v1/projects/{id}/extraction", [p("id", "Uuid", Kind::Path), p("extraction", "ProjectExtraction | null", Kind::Body)], body: Some("ProjectExtraction | null"), response: "Project",
        doc: "`null` drops the override and puts the project back on the global settings."),
    route!("projectLog", "GET", "/api/v1/projects/{id}/log", [
        p("id", "Uuid", Kind::Path),
        opt("filter", "LogFilter", Kind::QueryObject(&[("source", "source"), ("kind", "kind"), ("q", "q"), ("after", "after"), ("limit", "limit")])),
    ], body: None, response: "LogEntry[]",
        doc: "One page of the project's event history, newest first."),
    route!("projectLogExport", "GET", "/api/v1/projects/{id}/log/export", [p("id", "Uuid", Kind::Path)], body: None, response: "text",
        doc: "The whole log as JSONL, no filter and no cap. Returned as text, not parsed."),

    // ---- agents ----
    route!("listAgents", "GET", "/api/v1/agents", [], body: None, response: "Agent[]"),
    route!("saveAgent", "POST", "/api/v1/agents", [p("a", "NewAgent", Kind::Body)], body: Some("NewAgent"), response: "Agent"),
    route!("getAgent", "GET", "/api/v1/agents/{name}", [p("name", "string", Kind::Path)], body: None, response: "Agent"),
    route!("deleteAgent", "DELETE", "/api/v1/agents/{name}", [p("name", "string", Kind::Path)], body: None, response: "void"),

    // ---- practices: `api.ts` picks the collection from a `DocKind`, so these stay hand-written ----
    route!("listDocs", "GET", "/api/v1/practices", [PROJECT_Q], body: None, response: "Doc[]", client: Client::HandWritten),
    route!("saveDoc", "POST", "/api/v1/practices", [p("d", "NewDoc", Kind::Body)], body: Some("NewDoc"), response: "Doc", client: Client::HandWritten),
    route!("getDoc", "GET", "/api/v1/practices/{name}", [p("name", "string", Kind::Path)], body: None, response: "Doc", client: Client::HandWritten),
    route!("deleteDoc", "DELETE", "/api/v1/practices/{name}", [p("name", "string", Kind::Path)], body: None, response: "void", client: Client::HandWritten),

    // ---- workflows (graph editor) ----
    route!("listWorkflows", "GET", "/api/v1/workflows", [PROJECT_Q], body: None, response: "Workflow[]", actor: true),
    route!("createWorkflow", "POST", "/api/v1/workflows", [p("w", "NewWorkflow", Kind::Body)], body: Some("NewWorkflow"), response: "Workflow", actor: true),
    route!("getWorkflow", "GET", "/api/v1/workflows/{id}", [p("id", "Uuid", Kind::Path)], body: None, response: "Workflow", actor: true),
    route!("patchWorkflow", "PATCH", "/api/v1/workflows/{id}", [p("id", "Uuid", Kind::Path), p("patch", "WorkflowPatch", Kind::Body)], body: Some("WorkflowPatch"), response: "Workflow", actor: true,
        doc: "Only the fields in `patch` change."),
    route!("deleteWorkflow", "DELETE", "/api/v1/workflows/{id}", [p("id", "Uuid", Kind::Path)], body: None, response: "void", actor: true),
    route!("runWorkflow", "POST", "/api/v1/workflows/{id}/run", [
        p("id", "Uuid", Kind::Path),
        opt("trigger", "TriggerKind", Kind::Field("trigger")),
        opt("input", "string", Kind::Field("input")),
    ], body: Some("RunWorkflowBody"), response: "WorkflowRun", actor: true, client: Client::HandWritten,
        doc: "Hand-written: the desktop still passes `input` as `unknown` where the daemon reads a string."),
    route!("listRuns", "GET", "/api/v1/workflows/{id}/runs", [p("workflowId", "Uuid", Kind::Path), opt("limit", "number", Kind::Query("limit"))], body: None, response: "WorkflowRun[]", actor: true),
    route!("listAllRuns", "GET", "/api/v1/runs", [opt("since", "Timestamp", Kind::Query("since")), opt("limit", "number", Kind::Query("limit"))], body: None, response: "WorkflowRun[]",
        doc: "Every run across every workflow that finished after `since`, newest first."),
    route!("getRun", "GET", "/api/v1/runs/{id}", [p("id", "Uuid", Kind::Path)], body: None, response: "RunDetail", actor: true),
    route!("cancelRun", "POST", "/api/v1/runs/{id}/cancel", [p("id", "Uuid", Kind::Path)], body: None, response: "WorkflowRun", actor: true),
    route!("runExport", "GET", "/api/v1/runs/{id}/export", [p("id", "Uuid", Kind::Path)], body: None, response: "text",
        doc: "The run's log as JSONL, not parsed, like `projectLogExport`."),

    // ---- sync and settings ----
    route!("sync", "POST", "/api/v1/sync", [p("req", "SyncRequest", Kind::Body)], body: Some("SyncRequest"), response: "SyncReport"),
    route!("getSettings", "GET", "/api/v1/settings", [], body: None, response: "Settings"),
    route!("setSettings", "PUT", "/api/v1/settings", [p("partial", "Settings", Kind::Body)], body: Some("Settings"), response: "Settings"),

    // ---- extraction ----
    route!("ingest", "POST", "/api/v1/ingest", [p("text", "string", Kind::Field("text")), opt("projectRoot", "string", Kind::Field("project_root"))], body: Some("IngestBody"), response: "IngestReceipt", client: Client::HandWritten,
        doc: "Hand-written: the actor header carries the source tool, not `desktop`."),
    route!("getJob", "GET", "/api/v1/jobs/{id}", [p("id", "Uuid", Kind::Path)], body: None, response: "Job"),
    route!("testExtraction", "POST", "/api/v1/extraction/test", [PROJECT_Q], body: None, response: "ExtractionTestResult", client: Client::HandWritten,
        doc: "Hand-written: a 400 carries `{ok: false, error}` and is returned as data."),

    // ---- board ----
    route!("listTasks", "GET", "/api/v1/tasks", [
        opt("filter", "TaskFilter", Kind::QueryObject(&[
            ("project_id", "project_id"), ("stage", "stage"), ("assignee", "assignee"), ("persona", "persona"),
            ("ready", "ready"), ("q", "query"), ("include_done", "include_done"), ("top_level", "top_level"), ("brief", "brief"),
        ])),
    ], body: None, response: "Task[]", actor: true),
    route!("createTask", "POST", "/api/v1/tasks", [p("body", "NewTask", Kind::Body)], body: Some("NewTask"), response: "Task", actor: true),
    route!("taskCounts", "GET", "/api/v1/tasks/counts", [PROJECT_Q, opt("topLevel", "boolean", Kind::Query("top_level"))], body: None, response: "StageCount[]", actor: true),
    route!("getTask", "GET", "/api/v1/tasks/{id_or_key}", [p("key", "string", Kind::Path)], body: None, response: "TaskDetail", actor: true,
        doc: "The task plus its subtasks and its history. Takes a key or an id."),
    route!("updateTask", "PATCH", "/api/v1/tasks/{id_or_key}", [p("key", "string", Kind::Path), p("body", "TaskUpdate", Kind::Body)], body: Some("TaskUpdate"), response: "Task", actor: true,
        doc: "Only the fields in `body` change. `assignee: null` clears the assignee; leaving\nthe field out keeps it, which is what the daemon's double option means."),
    route!("deleteTask", "DELETE", "/api/v1/tasks/{id_or_key}", [p("key", "string", Kind::Path)], body: None, response: "void", actor: true),
    route!("moveTask", "POST", "/api/v1/tasks/{id_or_key}/move", [
        p("key", "string", Kind::Path),
        p("stage", "string", Kind::Field("stage")),
        opt("expectedUpdatedAt", "Timestamp", Kind::Field("expected_updated_at")),
    ], body: Some("MoveBody"), response: "Task", actor: true,
        doc: "`expectedUpdatedAt` turns a concurrent edit into a 409 instead of a clobber."),
    route!("commentTask", "POST", "/api/v1/tasks/{id_or_key}/comment", [p("key", "string", Kind::Path), p("body", "string", Kind::Field("body"))], body: Some("CommentBody"), response: "TaskEvent", actor: true),
    route!("claimTask", "POST", "/api/v1/tasks/{id_or_key}/claim", [p("key", "string", Kind::Path), opt("force", "boolean", Kind::Field("force"))], body: Some("ClaimBody"), response: "Task", actor: true,
        doc: "Assigns the task to this app's actor. `force` takes it from someone else."),
    route!("setTaskBlockers", "PUT", "/api/v1/tasks/{id_or_key}/blockers", [p("key", "string", Kind::Path), p("keys", "string[]", Kind::Field("blocked_by"))], body: Some("BlockersBody"), response: "Task", actor: true,
        doc: "Replaces the blocker list wholesale; an empty array clears it."),
    route!("boardStages", "GET", "/api/v1/board/stages", [PROJECT_Q], body: None, response: "StageList", actor: true,
        doc: "The stages in force: the project's own when it has some, the global list otherwise."),
    route!("setBoardStages", "PUT", "/api/v1/board/stages", [p("stages", "Stage[]", Kind::Field("stages")), opt("renames", "Record<string, string>", Kind::Field("renames"))], body: Some("SetStagesBody"), response: "Stage[]", actor: true,
        doc: "`renames` maps an old stage name to its new one, so the tasks in it follow."),
    route!("setProjectStages", "PUT", "/api/v1/projects/{id}/stages", [
        p("projectId", "Uuid", Kind::Path),
        p("stages", "Stage[] | null", Kind::Field("stages")),
        opt("renames", "Record<string, string>", Kind::Field("renames")),
    ], body: Some("SetProjectStagesBody"), response: "StageList", actor: true,
        doc: "`stages: null` drops the override and puts the project back on the global list."),

    // ---- frameworks ----
    route!("listFrameworks", "GET", "/api/v1/projects/{id}/frameworks", [p("projectId", "Uuid", Kind::Path)], body: None, response: "FrameworkListing[]",
        doc: "Every framework detected in the project, each with the documents it holds."),
    route!("getFrameworkDoc", "GET", "/api/v1/projects/{id}/frameworks/{kind}/docs/{*path}", [p("projectId", "Uuid", Kind::Path), p("kind", "FrameworkKind", Kind::Path), p("path", "string", Kind::Path)], body: None, response: "FrameworkDocContent", client: Client::HandWritten,
        doc: "Hand-written: `path` is encoded one segment at a time and the method answers the text."),
    route!("importFramework", "POST", "/api/v1/projects/{id}/frameworks/{kind}/import", [p("projectId", "Uuid", Kind::Path), p("kind", "FrameworkKind", Kind::Path), p("what", "ImportWhat", Kind::Field("what"))], body: Some("FrameworkImportBody"), response: "ImportReport", actor: true,
        doc: "Imports the framework's tasks onto the board, or its decisions as pending\nmemories. A write, recorded under this app's actor like every board request."),

    // ---- skills ----
    route!("listSkills", "GET", "/api/v1/skills", [PROJECT_Q], body: None, response: "SkillList",
        doc: "Every skill that applies: the Atlas-native ones plus the `SKILL.md` folders the\ndaemon discovers. With `projectId` the project's own roots are searched too and\neach row carries `enabled_here`."),
    route!("createSkill", "POST", "/api/v1/skills", [p("input", "NewSkill", Kind::Body)], body: Some("NewSkill"), response: "Skill",
        doc: "Creates an Atlas-native skill, global or scoped to one project."),
    route!("getSkill", "GET", "/api/v1/skills/{*id}", [p("id", "string", Kind::Path), PROJECT_Q], body: None, response: "Skill",
        doc: "One skill with its body and the other files in its folder."),
    route!("updateSkillBody", "PUT", "/api/v1/skills/{*id}", [p("id", "string", Kind::Path), p("body", "string", Kind::Field("body")), PROJECT_Q], body: Some("SkillBodyBody"), response: "Skill",
        doc: "Edit in place: rewrites a native skill's body, or the `SKILL.md` on disk. A\nproject-scoped discovered skill is only reachable with `projectId`, since the\ndaemon resolves the id against a listing of that project's roots taken right now."),
    route!("patchSkill", "PATCH", "/api/v1/skills/{*id}", [p("id", "string", Kind::Path), p("patch", "SkillPatch", Kind::Body)], body: Some("SkillPatch"), response: "Skill",
        doc: "Name and description of a native skill. Native ids are bare UUIDs, so this route\ntakes no `project_id`."),
    route!("deleteSkill", "DELETE", "/api/v1/skills/{*id}", [p("id", "string", Kind::Path)], body: None, response: "void",
        doc: "Native skills only; a discovered one belongs to the folder it came from. Native ids\nare bare UUIDs, so this route takes no `project_id` either."),
    route!("setProjectSkills", "PUT", "/api/v1/projects/{id}/skills", [p("id", "Uuid", Kind::Path), p("disabled", "string[]", Kind::Field("disabled"))], body: Some("SkillsDisabledBody"), response: "Project",
        doc: "Replaces the project's disabled skill list wholesale; an empty list clears it."),

    // ---- personas ----
    route!("listPersonas", "GET", "/api/v1/personas", [], body: None, response: "Persona[]"),
    route!("createPersona", "POST", "/api/v1/personas", [p("input", "NewPersona", Kind::Body)], body: Some("NewPersona"), response: "Persona", actor: true,
        doc: "Every persona write is audited against this app's actor, like a board write."),
    route!("getPersona", "GET", "/api/v1/personas/{id}", [p("idOrSlug", "string", Kind::Path)], body: None, response: "Persona",
        doc: "Takes an id, a slug or a name."),
    route!("updatePersona", "PUT", "/api/v1/personas/{id}", [p("id", "Uuid", Kind::Path), p("patch", "PersonaPatch", Kind::Body)], body: Some("PersonaPatch"), response: "Persona", actor: true,
        doc: "Only the fields in `patch` change. Takes the id, not the slug."),
    route!("deletePersona", "DELETE", "/api/v1/personas/{id}", [p("id", "Uuid", Kind::Path)], body: None, response: "void", actor: true),
    route!("getPersonaBundle", "GET", "/api/v1/personas/{id}/bundle", [p("id", "string", Kind::Path), PROJECT_Q], body: None, response: "PersonaBundle",
        doc: "The persona with everything it references resolved; missing references are warnings."),
    route!("getProjectRoster", "GET", "/api/v1/projects/{id}/personas", [p("projectId", "Uuid", Kind::Path)], body: None, response: "RosterRow[]"),
    route!("setProjectRoster", "PUT", "/api/v1/projects/{id}/personas", [p("projectId", "Uuid", Kind::Path), p("entries", "RosterEntry[]", Kind::Body)], body: Some("RosterEntry[]"), response: "RosterRow[]", actor: true,
        doc: "Replaces the project's roster wholesale: ids, the default and the order."),

    // ---- global search ----
    route!("globalSearch", "GET", "/api/v1/search", [
        p("q", "GlobalSearchQuery", Kind::QueryObject(&[("q", "q"), ("project_id", "project_id"), ("kinds", "kinds"), ("limit", "limit")])),
    ], body: None, response: "SearchResult",
        doc: "Cross-entity search: tasks, memories, projects, files, commits, events and\nworkflows in one call. Named `globalSearch` because `search` above is already\nthe memories recall route."),

    // ---- MCP ----
    route!("mcpStatus", "GET", "/api/v1/mcp/status", [], body: None, response: "McpStatusReport",
        doc: "Transports, counts, the tools table, resources, prompts and connected clients."),
    route!("registerMcpClient", "POST", "/api/v1/mcp/clients", [p("input", "RegisterMcpClientBody", Kind::Body)], body: Some("RegisterMcpClientBody"), response: "McpClient",
        doc: "The stdio shim's registration; the desktop never calls it."),
    route!("heartbeatMcpClient", "PUT", "/api/v1/mcp/clients/{id}", [p("id", "string", Kind::Path), p("toolCalls", "number", Kind::Field("tool_calls"))], body: Some("McpHeartbeatBody"), response: "McpClient"),
    route!("unregisterMcpClient", "DELETE", "/api/v1/mcp/clients/{id}", [p("id", "string", Kind::Path)], body: None, response: "void"),
    route!("projectMcp", "GET", "/api/v1/projects/{id}/mcp", [p("id", "Uuid", Kind::Path)], body: None, response: "ProjectMcpReport",
        doc: "What MCP looks like from one project's point of view: its tools table with\n`enabled_globally`/`enabled_here`, only this project's own resources, its\nprompts, the clients whose last call resolved here, and its connect info.\nTool gating applies at call time, not at the live tool list, so this is where a\nproject's own overrides show."),
    route!("setProjectMcpTools", "PUT", "/api/v1/projects/{id}/mcp/tools", [p("id", "Uuid", Kind::Path), p("disabled", "string[]", Kind::Field("disabled"))], body: Some("McpToolsBody"), response: "Project",
        doc: "Replaces the project's MCP tool override wholesale; an empty list clears it."),
    route!("listPluginTools", "GET", "/api/v1/mcp/plugin-tools", [], body: None, response: "PluginToolDecl[]"),
    route!("putPluginTools", "PUT", "/api/v1/mcp/plugin-tools/{plugin_id}", [p("pluginId", "string", Kind::Path), p("tools", "PluginToolDecl[]", Kind::Field("tools"))], body: Some("PluginToolsBody"), response: "void",
        doc: "Replaces the plugin's whole tool set; each decl's `plugin_id` is taken from the path."),
    route!("deletePluginTools", "DELETE", "/api/v1/mcp/plugin-tools/{plugin_id}", [p("pluginId", "string", Kind::Path)], body: None, response: "void"),
    route!("callPluginTool", "POST", "/api/v1/mcp/plugin-tools/{plugin_id}/{name}/call", [p("pluginId", "string", Kind::Path), p("name", "string", Kind::Path), opt("args", "unknown", Kind::Field("args"))], body: Some("PluginToolCallBody"), response: "unknown", actor: true),
    route!("pluginChannel", "GET", "/api/v1/mcp/plugin-channel", [], body: None, response: "void", client: Client::None,
        doc: "The plugin tool channel; `plugins/tools.ts` opens it as a `WebSocket`."),

    // ---- the agents' MCP servers ----
    route!("listMcpServers", "GET", "/api/v1/mcp/servers", [PROJECT_Q], body: None, response: "McpServerList",
        doc: "Every MCP server the user's agents are wired to. With no project that is the\nuser-scope entries of each agent; with one it is that project's own scopes plus\nthe plugin servers and Atlas, which apply everywhere."),
    route!("addMcpServer", "POST", "/api/v1/mcp/servers", [p("input", "NewMcpServer", Kind::Body)], body: Some("NewMcpServer"), response: "McpServerEntry",
        doc: "Writes a new entry into the agent's own config file."),
    route!("checkMcpServer", "POST", "/api/v1/mcp/servers/{id}/check", [p("id", "string", Kind::Path), PROJECT_Q], body: None, response: "McpCheckResult",
        doc: "Starts the server the way its agent would and asks it for its tools."),
    route!("setMcpServerEnabled", "PUT", "/api/v1/mcp/servers/{id}/enabled", [p("id", "string", Kind::Path), p("enabled", "boolean", Kind::Field("enabled")), PROJECT_Q], body: Some("McpEnabledBody"), response: "void",
        doc: "Flips the agent's own switch for one server; only offered where it has one."),
    route!("removeMcpServer", "DELETE", "/api/v1/mcp/servers/{id}", [p("id", "string", Kind::Path), PROJECT_Q], body: None, response: "void",
        doc: "Deletes the entry from the config file it came from."),
];
