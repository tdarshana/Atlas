// Generated from the `schemars::JsonSchema` derives in crates/atlas-core/src/models.rs,
// crates/atlas-core/src/search/global.rs and crates/atlas-core/src/jobs.rs by
// crates/atlas-core/src/tsgen.rs. Do not edit.
// Regenerate with: ATLAS_WRITE_TS=1 cargo test -p atlas-core --lib tsgen
//
// UUIDs and `DateTime<Utc>` both travel as strings; a `str_enum!` is its literal union.

export type Uuid = string;
/** RFC 3339, e.g. "2026-09-02T10:30:00Z". */
export type Timestamp = string;

// ---- shapes the daemon answers with (every field present, Option is `T | null`) ----

export type MemoryScope = 'global' | 'project';

export type MemoryKind = 'fact' | 'decision' | 'preference' | 'insight' | 'todo';

export type MemoryStatus = 'active' | 'pending' | 'rejected' | 'superseded';

export type MemoryScopeFilter = 'all' | 'project_only' | 'global_only';

export interface Memory {
	id: Uuid;
	scope: MemoryScope;
	project_id: Uuid | null;
	kind: MemoryKind;
	text: string;
	tags: string[];
	source_agent: string | null;
	source_tool: string | null;
	confidence: number;
	status: MemoryStatus;
	superseded_by: Uuid | null;
	created_at: Timestamp;
	updated_at: Timestamp;
}

export interface RecallHit {
	memory: Memory;
	score: number;
}

/**
 * Kind and tag counts, plus the total, over the active memories `GET
 * /memories/facets` was asked about; filtered the same way `GET /memories` filters
 * `project_id`, via `MemoryScopeFilter`. A side panel renders these without loading
 * every matching memory first.
 */
export interface MemoryFacets {
	kinds: Record<string, number>;
	tags: Record<string, number>;
	total: number;
}

export interface StatusReport {
	version: string;
	db_path: string;
	memories_active: number;
	memories_pending: number;
	embedding: string;
	/**
	 * The embedding model in use (the configured `embedding.model` or the default), so
	 * a client can show it even when the setting is unset.
	 */
	embedding_model: string;
	port: number | null;
	/**
	 * Lower-case hex SHA-256 of the daemon token (SEC-5). Only the daemon sets it; a
	 * local backend has no token and leaves it out.
	 */
	token_sha256?: string | null;
}

export type FrameworkKind = 'superpowers' | 'openspec' | 'speckit' | 'gsd';

/**
 * What `FrameworkAdapter::detect` found for one framework: which of its roots
 * exist under the project, and shallow, listing-only counts of what it holds.
 * Stored on `ProjectProfile.planning_frameworks` on connect and refresh.
 */
export interface FrameworkInventory {
	kind: FrameworkKind;
	/** Existing root paths for this framework, relative to the project root. */
	roots: string[];
	/** Matching document files by name and extension. Equal to `documents(root).len()`. */
	docs: number;
	/**
	 * Files that hold tasks (a `tasks.md`, a plan file, and so on), counted by
	 * name and extension, not by opening them: `detect` never reads a document,
	 * so this is a file count, not the exact number of importable checkbox
	 * items — call `tasks(root)` for that.
	 */
	tasks: number;
	detected_at: Timestamp;
}

export type FrameworkDocType = 'spec' | 'plan' | 'tasks' | 'roadmap' | 'ledger' | 'proposal' | 'summary' | 'todo';

/** One document a framework adapter found: a spec, plan, ledger and so on. */
export interface FrameworkDoc {
	kind: FrameworkKind;
	/** Path relative to the project root, as passed back to `FrameworkAdapter::read`. */
	path: string;
	title: string;
	doc_type: FrameworkDocType;
	updated_at: Timestamp;
}

/**
 * Points an imported task or decision back at the framework file it came from.
 * `anchor` names where inside that file: a heading, a ruling label, or similarly
 * a short human-readable locator, empty when the whole file is the source.
 */
export interface SourceRef {
	framework: FrameworkKind;
	path: string;
	anchor: string;
}

/**
 * One framework's inventory plus the documents it holds, the shape
 * `GET /api/v1/projects/{id}/frameworks` and the MCP `framework_docs` tool answer
 * with for each framework detected in a project.
 */
export interface FrameworkListing {
	inventory: FrameworkInventory;
	documents: FrameworkDoc[];
}

export type ImportWhat = 'tasks' | 'decisions';

/**
 * The tally `import::import_tasks` and `import::import_decisions` answer with:
 * how many items were newly created, how many existing ones were refreshed, and
 * how many were left alone because nothing about them had changed (or, for a
 * decision, because its text already exists).
 */
export interface ImportReport {
	created: number;
	updated: number;
	skipped: number;
	/**
	 * How many existing tasks were given a different parent (or had one removed) by
	 * this import. Counted independently of `updated`: a task whose parent changed
	 * but whose title and description did not is still `reparented`, not `updated`.
	 */
	reparented: number;
}

/**
 * A task line found by `FrameworkAdapter::tasks`, ready to become (or update) a
 * board task under `import::import_tasks`.
 */
export interface ImportedTask {
	title: string;
	description: string;
	/**
	 * Free-text status as the framework spelled it (for example `done`, `todo`),
	 * left to `import::import_tasks` to map onto a board stage.
	 */
	status_hint: string | null;
	source_ref: SourceRef;
	/**
	 * The parent task's own `source_ref.anchor`, when this item is a subtask of
	 * another `ImportedTask` from the same adapter run. `None` for a top-level item.
	 */
	parent_anchor: string | null;
}

/**
 * A decision line found by `FrameworkAdapter::decisions`, ready to become a
 * pending memory under `import::import_decisions`.
 */
export interface ImportedDecision {
	text: string;
	source_ref: SourceRef;
}

export interface ProjectProfile {
	name: string;
	languages: string[];
	frameworks: string[];
	tree: string[];
	readme_head: string;
	recent_commits: string[];
	summary: string | null;
	built_at: Timestamp;
	/**
	 * Planning frameworks (Superpowers, OpenSpec, SpecKit, GSD) detected in the
	 * project, distinct from the code frameworks above. `serde(default)` so a
	 * profile stored before this field existed still deserialises.
	 */
	planning_frameworks: FrameworkInventory[];
}

export interface Project {
	id: Uuid;
	name: string;
	root_path: string;
	git_remote: string | null;
	profile: ProjectProfile | null;
	created_at: Timestamp;
	last_seen_at: Timestamp;
	/**
	 * Board key prefix for this project's task keys (`ATL` in `ATL-12`). Null on
	 * rows written before migration 3; `TaskRepo::next_key` fills it in on first use.
	 */
	board_key: string | null;
	/** Per-project stage list. `None` means "use the global list". */
	board_stages: Stage[] | null;
	/** Which agent labels may write here. All-null by default, which lets anyone write. */
	agent_access: AgentAccess;
	/**
	 * Per-project extraction override. `None` means "use the global settings".
	 * `api_key` is masked to `"***"` on every read, like the global setting.
	 */
	extraction: ProjectExtraction | null;
	/**
	 * MCP tool names disabled for this project on top of the global
	 * `mcp.disabled_tools` list. Empty means no project override.
	 */
	mcp_disabled_tools: string[];
	/**
	 * Skill ids switched off for this project. Empty means every skill that applies
	 * here is on. The ids are [`SkillSummary::id`] values, so a discovered skill keeps
	 * its meaning across restarts.
	 */
	skills_disabled: string[];
}

/**
 * Who may write to a project, by actor label. `None` means any actor; a list is an
 * allow-list matched against the full actor string (`claude-code/reviewer`) or the
 * part before the slash (`claude-code`). The user's own hands are always exempt:
 * see [`crate::projects::actor_is_user`]. A project's unset field falls back to the
 * global `access.*` settings default (`require_review` as a floor it can only raise):
 * see [`crate::projects::effective_access`].
 */
export interface AgentAccess {
	memory_writers: string[] | null;
	task_movers: string[] | null;
	/** Forces a memory written here by an agent to land `pending` instead of `active`. */
	require_review: boolean;
}

/**
 * A project's access rules in all three shapes at once: its own `agent_access`, the
 * global `access.*` defaults, and the two resolved together (`crate::projects::effective_access`).
 * `GET /api/v1/projects/{id}/access` answers with this so a client can show the rule
 * that actually applies without also fetching `GET /api/v1/settings` to compute it.
 */
export interface ProjectAccess {
	access: AgentAccess;
	defaults: AgentAccess;
	effective: AgentAccess;
}

/**
 * What a log entry points at: a task (with its key), a memory, a job, a project or a
 * sync target.
 */
export interface LogRef {
	type: string;
	id: Uuid | null;
	key: string | null;
}

/**
 * One line of a project's unified log: task events, memory audit rows, project and
 * sync audit rows, and extraction jobs, merged and sorted newest first.
 */
export interface LogEntry {
	time: Timestamp;
	source: string;
	kind: string;
	detail: string;
	ref: LogRef | null;
}

/**
 * Everything an agent needs to start work in a project: the project itself,
 * the memories worth reading first, and the practices, workflows and skills in scope.
 */
export interface ProjectContext {
	project: Project;
	memories: RecallHit[];
	practices: Doc[];
	workflows: WorkflowSummary[];
	/**
	 * The skills that apply here, minus the ones this project switched off, so an
	 * agent reading its context knows which skills are actually in play.
	 */
	skills: SkillSummary[];
	/**
	 * The project's persona roster and the session's current persona, filled in by
	 * the MCP router (which is what holds a session); the daemon route leaves it out.
	 */
	personas?: PersonaContext | null;
}

export type DocKind = 'practice' | 'workflow';

export interface Doc {
	id: Uuid;
	kind: DocKind;
	name: string;
	body: string;
	tags: string[];
	project_id: Uuid | null;
	created_at: Timestamp;
	updated_at: Timestamp;
}

export type SyncKind = 'claude' | 'codex' | 'agents_md' | 'claude_md' | 'claude_hook' | 'codex_hook' | 'tasks_md' | 'framework_instructions';

export type SyncAction = 'Create' | 'Update' | 'Unchanged' | {
	Skip: string;
};

export interface SyncOp {
	kind: SyncKind;
	path: string;
	content: string;
	action: SyncAction;
	/**
	 * True when applying the op removes `path` instead of writing `content`: a
	 * persona export whose persona left the roster. Carried as a flag beside an
	 * `Update` action rather than as a variant of `SyncAction`, so every reader that
	 * matches the action keeps compiling and `--check` still exits non-zero for it.
	 */
	delete?: boolean;
}

export interface SyncReport {
	ops: SyncOp[];
	created: number;
	updated: number;
	unchanged: number;
	skipped: number;
	/** Persona exports removed because their persona left the roster. */
	deleted?: number;
}

export type TaskKind = 'task' | 'bug' | 'feature' | 'chore' | 'epic' | 'feature_request';

export type TaskPriority = 'low' | 'medium' | 'high' | 'urgent';

/**
 * One column of the board. `done` marks the terminal columns: moving into one
 * stamps `closed_at`, moving out clears it.
 */
export interface Stage {
	name: string;
	done: boolean;
}

/** A resolved stage list plus whether it came from a project override. */
export interface StageList {
	stages: Stage[];
	overridden: boolean;
}

export interface Task {
	id: Uuid;
	key: string;
	project_id: Uuid | null;
	seq: number;
	/**
	 * Where the task sits among its column's cards: cards sort by this, then `seq`.
	 * Backfilled from `seq`, so an untouched board keeps creation order; a drag sets a
	 * fraction between its new neighbours.
	 */
	position: number;
	title: string;
	description: string;
	stage: string;
	kind: TaskKind;
	priority: TaskPriority;
	assignee: string | null;
	labels: string[];
	parent_id: Uuid | null;
	/**
	 * The parent's key and title when this is a subtask, read with the row, so a
	 * board card can name its parent even when the parent is filtered out of the
	 * same listing.
	 */
	parent_key: string | null;
	parent_title: string | null;
	/**
	 * The persona this task is done as, read with the row the way `parent_key` is,
	 * so a card can show the role without a second lookup. All three are `None` for
	 * a task with no persona, and left off the wire then, so a task JSON written
	 * before personas existed still reads as one.
	 */
	persona_id?: Uuid | null;
	persona_name?: string | null;
	persona_slug?: string | null;
	created_by: string;
	created_at: Timestamp;
	updated_at: Timestamp;
	closed_at: Timestamp | null;
	/**
	 * Where this task was imported from, when it was. `import::import_tasks` looks
	 * tasks up by this field so a re-import updates rather than duplicates.
	 */
	source_ref: SourceRef | null;
	/** Keys of the tasks this one waits on. */
	blocked_by: string[];
	/**
	 * How many of `blocked_by` are not themselves in a done stage. The ready rule
	 * counts these and not the rest, so a badge drawn from this number agrees with
	 * `ready` instead of counting blockers that are already finished.
	 */
	open_blockers: number;
	/**
	 * Computed on read, never stored: not in a done stage, every blocker done,
	 * and no open subtask.
	 */
	ready: boolean;
	/** Why `ready` is false, when the task is open but held up. */
	blocked_reason: string | null;
	/**
	 * How many direct subtasks this task has. Computed on read, the same way
	 * `open_blockers` and `ready` are.
	 */
	subtasks_total: number;
	/** How many of `subtasks_total` sit in a done stage. */
	subtasks_done: number;
}

export interface TaskEvent {
	id: Uuid;
	task_id: Uuid;
	actor: string;
	/** created, edited, moved, assigned, commented, blocked, unblocked, deleted. */
	kind: string;
	body: string;
	detail: unknown;
	created_at: Timestamp;
}

export interface TaskDetail {
	task: Task;
	children: Task[];
	events: TaskEvent[];
}

/**
 * One write the daemon has just made, published on `GET /api/v1/events` the moment
 * it lands, so a client can refresh what it shows without polling. Every board write
 * records a task event and every other write an audit row, and those two writers are
 * where a change is published from, so nothing that reaches the database is missed.
 */
export interface Change {
	/**
	 * What kind of row changed: `task`, `memory`, `project`, `agent`, and so on, as the
	 * audit row or task event names it.
	 */
	entity: string;
	/** What happened, as the writer names it: `created`, `moved`, `insert`, `set_status`. */
	action: string;
	id: Uuid | null;
	/** The task key, when the change is to a task. */
	key: string | null;
	project_id: Uuid | null;
	at: Timestamp;
}

export type TriggerKind = 'manual' | 'schedule' | 'prompt';

export type NodeKind = 'trigger' | 'action' | 'output';

export type RunStatus = 'queued' | 'running' | 'success' | 'failed' | 'cancelled';

export type StepStatus = 'queued' | 'running' | 'success' | 'failed' | 'skipped' | 'cancelled';

export type LogLevel = 'INFO' | 'WARN' | 'ERR';

/**
 * What starts a workflow. `cron` is read only when `kind` is `schedule` and `prompt`
 * only when it is `prompt`; both are carried on every trigger so the editor can keep
 * a half-typed value while the user switches kinds.
 */
export interface Trigger {
	kind: TriggerKind;
	cron: string | null;
	prompt: string | null;
}

export interface Position {
	x: number;
	y: number;
}

/**
 * Which memories an action is given. Empty `kinds` or `tags` mean "no filter on
 * that axis", not "match nothing".
 */
export interface MemorySource {
	kinds: string[];
	tags: string[];
	limit: number;
	project_id: Uuid | null;
}

/**
 * The body of a node, shaped by the node's `kind`. Untagged rather than tagged: the
 * three shapes have disjoint required fields (`kind` / `name` / `propose_memories`),
 * the editor sends the plain object the GUI holds, and `graph::validate` is what
 * checks the variant against the node's declared `kind`.
 */
export type NodeData = Trigger | {
	name: string;
	instructions: string;
	agent: string;
	practices: string[];
	memories: MemorySource | null;
	/**
	 * Which of a persona's models this action runs on when the run is attributed
	 * to a persona. Off the wire when unset, so an existing graph reads as before.
	 */
	case?: Case | null;
} | {
	propose_memories: boolean;
	file_tasks: boolean;
};

export interface Node {
	id: string;
	kind: NodeKind;
	position: Position;
	data: NodeData;
}

export interface Edge {
	id: string;
	source: string;
	target: string;
}

export interface Graph {
	nodes: Node[];
	edges: Edge[];
}

export interface Workflow {
	id: Uuid;
	name: string;
	project_id: Uuid | null;
	description: string;
	trigger: Trigger;
	graph: Graph;
	enabled: boolean;
	created_at: Timestamp;
	updated_at: Timestamp;
	last_run_at: Timestamp | null;
	last_status: RunStatus | null;
}

/**
 * A workflow's shape without its graph: what an agent (over MCP) or a project's
 * context needs to decide whether a workflow is worth looking at closer, without the
 * weight of its full node/edge JSON. `get_workflow` (MCP) and the desktop's own
 * `GET /api/v1/workflows/{id}` still answer with the full [`Workflow`], graph
 * included.
 */
export interface WorkflowSummary {
	id: Uuid;
	name: string;
	trigger: TriggerKind;
	action_count: number;
	enabled: boolean;
	last_status: RunStatus | null;
}

export interface WorkflowRun {
	id: Uuid;
	workflow_id: Uuid;
	/** Per-workflow, starting at 1: the number a run is known by in the GUI and CLI. */
	number: number;
	trigger: TriggerKind;
	status: RunStatus;
	started_at: Timestamp;
	finished_at: Timestamp | null;
	summary: unknown;
}

export interface LogLine {
	ts: Timestamp;
	level: LogLevel;
	text: string;
}

export interface WorkflowStep {
	id: Uuid;
	run_id: Uuid;
	/** Zero-based index in the run's execution order. */
	position: number;
	/** The graph node this step ran, so a step can be traced back to the canvas. */
	action_id: string;
	name: string;
	agent: string;
	status: StepStatus;
	started_at: Timestamp;
	finished_at: Timestamp | null;
	output: string | null;
	log: LogLine[];
}

export type SearchKind = 'task' | 'memory' | 'project' | 'file' | 'commit' | 'event' | 'workflow';

export interface SearchHit {
	kind: SearchKind;
	id: string;
	title: string;
	subtitle: string | null;
	project_id: Uuid | null;
	reference: string | null;
	score: number;
	/**
	 * Char offsets `(start, end)` into `title` of the first case-insensitive match of
	 * the query, or empty when the hit scored on a field other than `title`.
	 */
	highlights: [number, number][];
}

export interface SearchGroup {
	kind: SearchKind;
	items: SearchHit[];
}

export interface SearchResult {
	groups: SearchGroup[];
	total: number;
	took_ms: number;
}

/**
 * Whether a plugin tool only reads state or can change it. Mirrors `atlas-mcp`'s
 * `ToolScope`, which this crate cannot import: `atlas-mcp` depends on `atlas-core`,
 * not the other way round. Serialised the same way (`"read"` / `"write"`), so the
 * `/api/v1/mcp/status` tools table renders a plugin row's badge exactly like a
 * built-in's.
 */
export type PluginToolScope = 'read' | 'write';

/**
 * One MCP tool a desktop plugin contributes. The app registers a plugin's whole set
 * with `PUT /api/v1/mcp/plugin-tools/{plugin_id}`, where the body omits `plugin_id`
 * (the path already names it) and the daemon fills it in, which is why the field
 * defaults rather than being required.
 */
export interface PluginToolDecl {
	plugin_id: string;
	name: string;
	description: string;
	/**
	 * A JSON Schema object describing the tool's arguments, handed to MCP clients as
	 * the tool's `inputSchema` unchanged.
	 */
	args: unknown;
	scope: PluginToolScope;
}

export type McpServerSource = 'claude' | 'codex' | 'cursor' | 'gemini' | 'windsurf' | 'plugin' | 'atlas';

export type McpServerScope = 'user' | 'project' | 'local' | 'plugin';

/**
 * How a server is started or reached, with secrets reduced to key names. The values of
 * `env` and `headers` stay in the file they came from: [`McpTransport`] carries only
 * `env_keys` and `header_keys`, so a listing can say what a server needs without ever
 * putting a token on the wire.
 */
export type McpTransport = {
	command: string;
	args: string[];
	env_keys: string[];
	kind: 'stdio';
} | {
	url: string;
	header_keys: string[];
	kind: 'http';
};

/**
 * One MCP server as a listing shows it.
 *
 * `id` is `"<source>:<scope>:<name>"`, with `plugin:<marketplace>/<plugin>:<name>` for a
 * plugin server and the bare `atlas` for Atlas's own. Ids are stable across restarts, so
 * a desktop row keeps its meaning.
 */
export interface McpServerEntry {
	id: string;
	/** The key the agent's own configuration files it under. */
	name: string;
	source: McpServerSource;
	scope: McpServerScope;
	transport: McpTransport;
	/**
	 * Absolute path of the configuration file this entry was read from. `None` for
	 * Atlas's own synthesised entry, which no file declares.
	 */
	file: string | null;
	/** `"<marketplace>/<plugin>"` for a plugin server, `None` otherwise. */
	plugin: string | null;
	/** Whether the agent will actually start this server, as its own configuration says. */
	enabled: boolean;
	/**
	 * Whether the agent has a native switch Atlas can flip. Where it is false, `Remove`
	 * is the only way to stop a server.
	 */
	can_toggle: boolean;
	/** Whether Atlas can delete this entry from the file it came from. */
	can_remove: boolean;
	/** Atlas's own server, which the desktop renders with its client and gating detail. */
	is_atlas: boolean;
	/** The project a `project` or `local` scoped entry belongs to. */
	project_id: Uuid | null;
}

/**
 * A server listing plus whatever discovery could not read. A warning never fails the
 * listing: one unreadable agent config must not hide every other server.
 */
export interface McpServerList {
	servers: McpServerEntry[];
	warnings: string[];
}

/** One tool a checked server reported. */
export interface McpToolInfo {
	name: string;
	description: string | null;
}

/**
 * What starting a server and asking it for its tools found. `ok: false` carries the
 * reason in `error` rather than failing the call: a server that will not start is an
 * answer, not a broken request.
 */
export interface McpCheckResult {
	ok: boolean;
	server_name: string | null;
	server_version: string | null;
	protocol_version: string | null;
	tools: McpToolInfo[];
	error: string | null;
	elapsed_ms: number;
}

export type SkillSource = 'native' | 'claude-project' | 'claude-user' | 'codex-project' | 'codex-user' | 'plugin';

/**
 * One skill as a listing shows it, without its body.
 *
 * `id` is `"<source>:<path of the skill folder relative to its source root>"` for a
 * discovered skill (a plugin skill's root is the plugin's `skills` directory, so its id
 * is `plugin:<marketplace>/<plugin>/<skill>` and survives the plugin being updated into
 * a new version directory), and the UUID for a native one. Ids are stable across
 * restarts, so a project's disabled list keeps its meaning.
 */
export interface SkillSummary {
	id: string;
	source: SkillSource;
	name: string;
	description: string;
	/**
	 * `global` for a user, plugin or project-less native skill; `project` for one that
	 * belongs to a single project. The same two words `MemoryScope` uses.
	 */
	scope: MemoryScope;
	project_id: Uuid | null;
	/** Absolute path of the skill folder, for a discovered skill. `None` for a native one. */
	path: string | null;
	/** `"<marketplace>/<plugin>"` for a plugin skill, `None` otherwise. */
	plugin: string | null;
	/**
	 * Whether Atlas can write this skill's body: every native skill, and every
	 * discovered one whose `SKILL.md` the daemon user may write.
	 */
	editable: boolean;
	updated_at: Timestamp | null;
	/**
	 * Whether the skill is on for the project a listing was asked about. `None` when
	 * no project was given.
	 */
	enabled_here: boolean | null;
}

/**
 * One skill with its text: the whole `SKILL.md` (frontmatter included) for a
 * discovered skill, the stored body for a native one.
 */
export interface Skill {
	id: string;
	source: SkillSource;
	name: string;
	description: string;
	/**
	 * `global` for a user, plugin or project-less native skill; `project` for one that
	 * belongs to a single project. The same two words `MemoryScope` uses.
	 */
	scope: MemoryScope;
	project_id: Uuid | null;
	/** Absolute path of the skill folder, for a discovered skill. `None` for a native one. */
	path: string | null;
	/** `"<marketplace>/<plugin>"` for a plugin skill, `None` otherwise. */
	plugin: string | null;
	/**
	 * Whether Atlas can write this skill's body: every native skill, and every
	 * discovered one whose `SKILL.md` the daemon user may write.
	 */
	editable: boolean;
	updated_at: Timestamp | null;
	/**
	 * Whether the skill is on for the project a listing was asked about. `None` when
	 * no project was given.
	 */
	enabled_here: boolean | null;
	body: string;
	/**
	 * The other files in the skill folder, relative to it, at most 200, for display
	 * only: nothing reads or writes them.
	 */
	files: string[];
}

/**
 * A skill listing plus whatever discovery could not read. A warning never fails the
 * listing: one unreadable folder must not hide every other skill.
 */
export interface SkillList {
	skills: SkillSummary[];
	warnings: string[];
}

export type Case = 'plan' | 'implement' | 'review' | 'test' | 'document' | 'default';

export type PersonaRule = 'allow' | 'deny' | 'review';

/**
 * A persona's own write permissions, applied after the project's agent rules with the
 * stricter answer winning. Every rule defaults to `allow`.
 */
export interface PersonaAccess {
	memory_write: PersonaRule;
	task_move: PersonaRule;
	workflow_trigger: PersonaRule;
}

/**
 * A library persona: a role an agent adopts, bundling what it works with and how.
 * Global and unique by name (compared without case); `slug` is derived from the name
 * and is the export file name and the `agent_use` key.
 */
export interface Persona {
	id: Uuid;
	name: string;
	slug: string;
	/** One line, the job title shown on chips. */
	role: string;
	/** One paragraph, shown in rosters and in `project_context`. */
	summary: string;
	/** Markdown: decision style, preferences, rules. */
	instructions: string;
	/** Skill ids as `skill_list` names them. */
	skills: string[];
	/** Workflow names, global or project. */
	workflows: string[];
	/** Practice ids. */
	practices: string[];
	/** MCP server ids as `GET /mcp/servers` names them. */
	mcp_servers: string[];
	/** Atlas MCP tool names a session may see; empty means all. */
	tools: string[];
	access: PersonaAccess;
	/** A model name per case, any subset of the six. */
	models: {
		default?: string;
		document?: string;
		implement?: string;
		plan?: string;
		review?: string;
		test?: string;
	};
	tags: string[];
	created_at: Timestamp;
	updated_at: Timestamp;
}

/**
 * One line of a project's roster as it reads back: the persona's summary fields
 * with its place on this project.
 */
export interface RosterRow {
	persona_id: Uuid;
	name: string;
	slug: string;
	role: string;
	summary: string;
	tags: string[];
	is_default: boolean;
	position: number;
	project_id: Uuid;
}

/**
 * A persona with everything it references resolved. A reference that no longer
 * resolves is a warning here, never an error: a persona keeps working while a plugin
 * is being reinstalled.
 */
export interface PersonaBundle {
	persona: Persona;
	skills: SkillSummary[];
	workflows: WorkflowSummary[];
	practices: Doc[];
	mcp_servers: McpServerEntry[];
	warnings: string[];
}

/**
 * The personas in play for a project, as `project_context` reports them: the
 * roster, its default and the persona the calling session has adopted (or `None`).
 */
export interface PersonaContext {
	roster: RosterRow[];
	default: RosterRow | null;
	current: PersonaBundle | null;
}

/**
 * `Deserialize` as well as `Serialize`, so `RemoteBackend` can read a job back
 * off `GET /jobs/{id}` rather than re-describing the shape; `JsonSchema` so `tsgen`
 * types it for the desktop. `status` is `queued`, `running`, `done` or `failed`.
 */
export interface Job {
	id: Uuid;
	kind: string;
	status: string;
	payload: unknown;
	result: unknown;
	error: string | null;
	created_at: Timestamp;
	updated_at: Timestamp;
}

/** `POST /api/v1/extraction/test`: 200 carries `reply`, the 400 connectivity failure carries `error`. */
export interface ExtractionTestResult {
	ok: boolean;
	reply?: string | null;
	error?: string | null;
}

/** `POST /api/v1/ingest`'s 202 body: the job to follow with `GET /api/v1/jobs/{id}`. */
export interface IngestReceipt {
	job_id: Uuid;
}

/** `GET /api/v1/projects/{id}/frameworks/{kind}/docs/{path}`: one document's text. */
export interface FrameworkDocContent {
	content: string;
}

/** One row of `GET /api/v1/tasks/counts`; every stage appears, including empty ones. */
export interface StageCount {
	stage: string;
	count: number;
}

/** `GET /api/v1/runs/{id}`. */
export interface RunDetail {
	run: WorkflowRun;
	steps: WorkflowStep[];
}

/** Whether an MCP tool reads or writes. A project's agent-access rules gate the writes. */
export type McpToolScope = 'read' | 'write';

/**
 * One row of `GET /api/v1/mcp/status`'s tools table. `source` is `builtin` for a tool
 * the daemon carries itself, `plugin:<id>` for one a plugin registered.
 */
export interface McpToolRow {
	name: string;
	description: string;
	args: string;
	scope: McpToolScope;
	enabled: boolean;
	source: string;
}

/**
 * One tool as the MCP server exposes it from one project's point of view, built-in or
 * plugin, with the two gates a call meets reported separately: the global
 * `mcp.disabled_tools` list, then that project's own `mcp_disabled_tools` override.
 * `enabled_here` is what a call actually gets; without a project it equals
 * `enabled_globally`. The desktop's badges need both, since a tool can be enabled
 * globally and disabled here. `GET /api/v1/projects/{id}/mcp`'s tool row, and the row
 * `atlas_mcp::effective_tools` computes, the same computation the router gates on.
 */
export interface ProjectMcpToolRow {
	name: string;
	description: string;
	/**
	 * A short, comma-joined summary of arguments, `*` marking a required one: the
	 * hand-written one from `TOOL_TABLE` for a built-in, the schema's own property
	 * names for a plugin's, since a plugin declares a JSON Schema instead.
	 */
	args: string;
	scope: McpToolScope;
	enabled_globally: boolean;
	/** Actually callable here: enabled globally and not in the project's own override. */
	enabled_here: boolean;
	/** `builtin`, or `plugin:<id>` for a tool a plugin contributed. */
	source: string;
}

export interface McpStdioTransport {
	command: string;
}

export interface McpHttpTransport {
	url: string;
	protocol_version: string;
}

export interface McpTransports {
	stdio: McpStdioTransport;
	http: McpHttpTransport;
}

export interface McpCounts {
	tools: number;
	resources: number;
	prompts: number;
	clients: number;
}

/** An MCP resource as `resources/list` shows it. */
export interface McpResource {
	uri: string;
	name: string;
	title?: string | null;
	description?: string | null;
	mimeType?: string | null;
	size?: number | null;
}

export interface McpPromptArgument {
	name: string;
	title?: string | null;
	description?: string | null;
	required?: boolean | null;
}

/** An MCP prompt as `prompts/list` shows it. */
export interface McpPrompt {
	name: string;
	title?: string | null;
	description?: string | null;
	arguments?: McpPromptArgument[] | null;
}

export type McpClientTransport = 'stdio' | 'http';

/**
 * One MCP client connected to the daemon: a stdio shim that registered itself, or an
 * HTTP session picked up on its first tool call.
 */
export interface McpClient {
	id: string;
	transport: McpClientTransport;
	client_name: string;
	client_version: string | null;
	first_seen: Timestamp;
	last_seen: Timestamp;
	tool_calls: number;
	/**
	 * The project this client's last tool call resolved, best effort: only an HTTP
	 * session reports one (it comes from the router's own project resolution on that
	 * call); null for a stdio session, or a call that named no project.
	 */
	last_project_id: Uuid | null;
}

/** `GET /api/v1/mcp/status`. */
export interface McpStatusReport {
	transports: McpTransports;
	counts: McpCounts;
	tools: McpToolRow[];
	resources: McpResource[];
	prompts: McpPrompt[];
	clients: McpClient[];
}

export interface ProjectMcpConnect {
	stdio: McpStdioTransport;
	http: McpHttpTransport;
	project_root: string;
}

/**
 * `GET /api/v1/projects/{id}/mcp`: MCP from one project's point of view. Tool gating
 * applies at call time, not at `tools/list`, so this route (not the live tool list) is
 * where a project's own MCP overrides show.
 */
export interface ProjectMcpReport {
	tools: ProjectMcpToolRow[];
	/** Only this project's own `atlas://` resources (its context, practices and board). */
	resources: McpResource[];
	prompts: McpPrompt[];
	/** Registry entries whose last call resolved to this project. */
	clients: McpClient[];
	connect: ProjectMcpConnect;
}

// ---- shapes the daemon reads (a defaulted or Option field may be left out) ----

export interface NewMemory {
	scope: MemoryScope;
	project_id?: Uuid | null;
	kind: MemoryKind;
	text: string;
	tags?: string[];
	source_agent?: string | null;
	source_tool?: string | null;
	confidence?: number;
	status?: MemoryStatus;
}

export interface RecallQuery {
	query: string;
	limit?: number;
	scope?: MemoryScope | null;
	/**
	 * How `project_id` is read, the same narrowing `GET /memories` takes: `All` widens
	 * to the project plus every global memory, `ProjectOnly` keeps the project's own.
	 */
	list_scope?: MemoryScopeFilter;
	project_id?: Uuid | null;
	kinds?: MemoryKind[];
	tags?: string[];
}

/**
 * A project's extraction override, with the same fields as the global
 * `extraction.*` settings. An absent field falls back to the global value, so a
 * project can point one endpoint somewhere else without restating the rest.
 */
export interface ProjectExtraction {
	enabled?: boolean | null;
	base_url?: string | null;
	model?: string | null;
	api_key?: string | null;
	auto_accept_min_confidence?: number | null;
}

/**
 * A patch to a project's identity. An absent field is left alone; `git_remote` is a
 * double option, so an explicit JSON `null` clears it.
 */
export interface ProjectPatch {
	name?: string | null;
	board_key?: string | null;
	git_remote?: string | null;
	/**
	 * Replaces the project's MCP tool override wholesale when present. Validated
	 * against the same known-tool-name list as the global `mcp.disabled_tools`.
	 */
	mcp_disabled_tools?: string[] | null;
}

/**
 * Narrows a project log. `after` pages: it keeps entries strictly older than the
 * given time, which is the last entry of the previous page.
 */
export interface LogFilter {
	source?: string | null;
	kind?: string | null;
	q?: string | null;
	after?: Timestamp | null;
	limit?: number | null;
}

export interface NewDoc {
	name: string;
	body: string;
	tags?: string[];
	project_id?: Uuid | null;
}

/**
 * One sync request. `root` is required unless `global` is set, in which case
 * the sync targets a home directory instead of a project: `ATLAS_SYNC_HOME`
 * when set on the daemon process, else the daemon user's own home. There is
 * no client-side override; `POST /sync` is unauthenticated, so the request
 * itself must not be able to name an arbitrary write target.
 */
export interface SyncRequest {
	root?: string | null;
	global?: boolean;
	targets?: SyncKind[];
	check_only?: boolean;
}

export interface NewTask {
	project_id?: Uuid | null;
	title: string;
	description?: string | null;
	kind?: TaskKind | null;
	priority?: TaskPriority | null;
	assignee?: string | null;
	labels?: string[] | null;
	/** Parent task, by id or key. */
	parent?: string | null;
	/** Blocking tasks, by id or key. */
	blocked_by?: string[] | null;
	stage?: string | null;
	/** Set by `import::import_tasks` so a re-import finds this task again. */
	source_ref?: SourceRef | null;
	/** The persona to do this task as, by id or slug. Empty is the same as absent. */
	persona?: string | null;
}

/**
 * A patch. An absent field is left alone. `assignee`, `parent` and `persona` are
 * double options so an explicit JSON `null` clears them: absent is `None`, `null` is
 * `Some(None)`, a value is `Some(Some(v))`. `persona` also clears on `""`.
 */
export interface TaskUpdate {
	title?: string | null;
	description?: string | null;
	kind?: TaskKind | null;
	priority?: TaskPriority | null;
	assignee?: string | null;
	labels?: string[] | null;
	parent?: string | null;
	/** The persona by id or slug; `null` or `""` clears it. */
	persona?: string | null;
	expected_updated_at?: Timestamp | null;
}

export interface TaskFilter {
	project_id?: Uuid | null;
	stage?: string | null;
	assignee?: string | null;
	/** Keep only tasks that are ready to be worked on. */
	ready?: boolean;
	/** Case-insensitive substring match over key, title and description. */
	query?: string | null;
	include_done?: boolean;
	/**
	 * Keep only tasks with no project at all: the literal global board, distinct from
	 * a bare `project_id: None`, which leaves every project's tasks in.
	 */
	global_only?: boolean;
	/**
	 * `Some(true)` keeps only tasks with no parent (`parent_id is null`); `Some(false)`
	 * keeps only subtasks; `None` applies no filter either way.
	 */
	top_level?: boolean | null;
	/** Keep only tasks done as this persona, by id or slug. */
	persona?: string | null;
	/**
	 * Leave `description` empty and `source_ref` off each row. For a listing that
	 * shows neither (the board), where the description is most of the bytes; `get`
	 * still answers with the full task. The filters, `query` included, are unchanged.
	 */
	brief?: boolean;
}

export interface NewWorkflow {
	name: string;
	project_id?: Uuid | null;
	description?: string;
	trigger: Trigger;
	graph?: Graph;
	enabled?: boolean;
}

/**
 * A patch. `project_id` is a double option so a caller can tell "leave the project
 * alone" from "make this workflow global", the same distinction `TaskUpdate` draws.
 */
export interface WorkflowPatch {
	name?: string | null;
	project_id?: Uuid | null;
	description?: string | null;
	trigger?: Trigger | null;
	graph?: Graph | null;
	enabled?: boolean | null;
}

export interface SearchQuery {
	q: string;
	project_id?: Uuid | null;
	kinds?: SearchKind[] | null;
	limit?: number;
}

/**
 * The transport of a server being added, with the secret values the file will hold.
 * This shape only ever travels inwards: a listing answers with [`McpTransport`], which
 * has key names and no values.
 */
export type McpTransportInput = {
	command: string;
	args?: string[];
	env?: Record<string, string>;
	kind: 'stdio';
} | {
	url: string;
	headers?: Record<string, string>;
	kind: 'http';
};

/**
 * A server to write into one agent's configuration. `project_id` is required for a
 * `project` or `local` scope; a `user` scope ignores it.
 */
export interface NewMcpServer {
	source: McpServerSource;
	scope: McpServerScope;
	project_id?: Uuid | null;
	name: string;
	transport: McpTransportInput;
}

/** A new Atlas-native skill. `project_id` scopes it to one project; `None` is global. */
export interface NewSkill {
	project_id?: Uuid | null;
	name: string;
	description?: string;
	body?: string;
}

/** A patch to a native skill. An absent field is left alone. */
export interface SkillUpdate {
	name?: string | null;
	description?: string | null;
	body?: string | null;
}

/** A persona to create. Everything but `name` has a default. */
export interface NewPersona {
	name: string;
	role?: string;
	summary?: string;
	instructions?: string;
	skills?: string[];
	workflows?: string[];
	practices?: string[];
	mcp_servers?: string[];
	tools?: string[];
	access?: PersonaAccess;
	models?: {
		default?: string;
		document?: string;
		implement?: string;
		plan?: string;
		review?: string;
		test?: string;
	};
	tags?: string[];
}

/** A patch to a persona. An absent field is left alone; a new `name` derives a new slug. */
export interface PersonaUpdate {
	name?: string | null;
	role?: string | null;
	summary?: string | null;
	instructions?: string | null;
	skills?: string[] | null;
	workflows?: string[] | null;
	practices?: string[] | null;
	mcp_servers?: string[] | null;
	tools?: string[] | null;
	access?: PersonaAccess | null;
	models?: {
		default?: string;
		document?: string;
		implement?: string;
		plan?: string;
		review?: string;
		test?: string;
	} | null;
	tags?: string[] | null;
}

/**
 * One line of a project's roster as a caller sets it. `PUT /projects/{id}/personas`
 * takes the whole list: ids, which one is the default, and the order.
 */
export interface RosterEntry {
	persona_id: Uuid;
	is_default?: boolean;
	position?: number;
}

export interface ForgetBody {
	reason?: string | null;
}

export interface RootBody {
	root: string;
}

export interface StatusBody {
	status: string;
}

export interface IngestBody {
	text: string;
	/**
	 * Deprecated: send the actor as `X-Atlas-Actor` instead. Kept for one release so
	 * an older caller still works; the header wins when both are sent.
	 */
	source_tool?: string | null;
	project_root?: string | null;
}

export interface MoveBody {
	stage: string;
	expected_updated_at?: Timestamp | null;
	/**
	 * Where to place the task in the target column (a drag's spot between neighbours);
	 * left out, a move keeps the task's position and a same-stage move does nothing.
	 */
	position?: number | null;
}

export interface CommentBody {
	body: string;
}

export interface ClaimBody {
	force?: boolean;
}

export interface BlockersBody {
	blocked_by: string[];
}

export interface SetStagesBody {
	stages: Stage[];
	renames?: Record<string, string>;
}

export interface SetProjectStagesBody {
	stages?: Stage[] | null;
	renames?: Record<string, string>;
}

export interface FrameworkImportBody {
	what: ImportWhat;
}

export interface SkillBodyBody {
	body: string;
}

export interface SkillsDisabledBody {
	disabled: string[];
}

export interface McpToolsBody {
	disabled: string[];
}

export interface McpEnabledBody {
	enabled: boolean;
}

export interface RunWorkflowBody {
	trigger?: TriggerKind | null;
	input?: string | null;
}

export interface RegisterMcpClientBody {
	id: string;
	transport: string;
	client_name: string;
	client_version?: string | null;
}

export interface McpHeartbeatBody {
	tool_calls: number;
}

/**
 * `PUT /api/v1/mcp/plugin-tools/{plugin_id}`'s body. Each decl's `plugin_id` is
 * optional in the JSON and overwritten from the path.
 */
export interface PluginToolsBody {
	tools: PluginToolDecl[];
}

export interface PluginToolCallBody {
	args?: unknown;
}
