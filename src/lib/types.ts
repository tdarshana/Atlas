// TypeScript mirrors of the wire shapes in crates/atlas-core/src/models.rs.
// UUIDs and `DateTime<Utc>` both arrive as strings; the string unions match the
// `str_enum!` literals exactly, so a value that type checks here parses there.

export type Uuid = string;
/** RFC 3339, e.g. "2026-09-02T10:30:00Z". */
export type Timestamp = string;

export type MemoryScope = 'global' | 'project';
export type MemoryKind = 'fact' | 'decision' | 'preference' | 'insight' | 'todo';
export type MemoryStatus = 'active' | 'pending' | 'rejected' | 'superseded';

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
 * A request-time narrowing, not a value of a memory's own `scope` column (that is
 * `MemoryScope`). `project_only` asks `GET /memories` and `POST /memories/search` for
 * exactly one project's own memories, nothing global; `global_only` narrows to the
 * project-less memories, ignoring any `project_id` sent alongside it; `all` is the
 * default, where a project widens to its own memories plus every global one.
 */
export type MemoryListScope = 'all' | 'project_only' | 'global_only';

/**
 * Kind and tag counts, plus the total, over the active memories `GET
 * /memories/facets` was asked about, `project_id`/`scope` read the same way `GET
 * /memories` reads them.
 */
export interface MemoryFacets {
	kinds: Record<string, number>;
	tags: Record<string, number>;
	total: number;
}

export interface RecallQuery {
	query: string;
	limit?: number;
	scope?: MemoryScope | null;
	/** How `project_id` is read. Defaults to `all` on the daemon. */
	list_scope?: MemoryListScope | null;
	project_id?: Uuid | null;
	kinds?: MemoryKind[];
	tags?: string[];
}

export interface StatusReport {
	version: string;
	db_path: string;
	memories_active: number;
	memories_pending: number;
	embedding: string;
	port: number | null;
}

/** The desktop app's `about_info` Tauri command. */
export interface AboutInfo {
	app_version: string;
	tauri_version: string;
	os_type: string;
	os_version: string;
	arch: string;
	locale: string | null;
	log_dir: string;
	data_dir: string;
}

/** `vault_status`: `missing` before `atlas.hold` exists, `locked` once it does but this
 * process has not opened it, `unlocked` while it holds the open vault in memory. */
export type VaultStatus = 'missing' | 'locked' | 'unlocked';

/** `update_check`'s reply. `available: false` means the current version is the latest;
 * a rejected promise (not this shape) means the check itself failed, including the
 * placeholder pubkey's "updates are not configured". */
export interface UpdateCheckResult {
	available: boolean;
	version: string | null;
	notes: string | null;
	date: string | null;
}

/** `atlas:update-progress`'s payload. `total` is absent when the server did not send a
 * content length. */
export interface UpdateProgress {
	downloaded: number;
	total: number | null;
}

/** A planning framework (Superpowers, OpenSpec, SpecKit, GSD), distinct from the
 * code frameworks (Svelte, Tauri, ...) in `ProjectProfile.frameworks`. */
export type FrameworkKind = 'superpowers' | 'openspec' | 'speckit' | 'gsd';

/** What detection found for one planning framework: its roots plus document and
 * task counts. */
export interface FrameworkInventory {
	kind: FrameworkKind;
	roots: string[];
	docs: number;
	tasks: number;
	detected_at: Timestamp;
}

export type FrameworkDocType = 'spec' | 'plan' | 'tasks' | 'roadmap' | 'ledger' | 'proposal' | 'summary' | 'todo';

/** One document a framework holds: a spec, plan, ledger and so on. `path` is what
 * `GET /projects/{id}/frameworks/{kind}/docs/{path}` and the MCP tool `framework_docs`
 * take to fetch its text. */
export interface FrameworkDoc {
	kind: FrameworkKind;
	path: string;
	title: string;
	doc_type: FrameworkDocType;
	updated_at: Timestamp;
}

/** Points an imported task or decision back at the framework file it came from.
 * `anchor` names where inside that file (a heading, a ruling label), empty when the
 * whole file is the source. */
export interface SourceRef {
	framework: FrameworkKind;
	path: string;
	anchor: string;
}

/** `GET /projects/{id}/frameworks`'s shape for one detected framework. */
export interface FrameworkListing {
	inventory: FrameworkInventory;
	documents: FrameworkDoc[];
}

export type ImportWhat = 'tasks' | 'decisions';

/** `POST /projects/{id}/frameworks/{kind}/import`'s reply. */
export interface ImportReport {
	created: number;
	updated: number;
	skipped: number;
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
	/** Optional: absent on a profile stored before this field existed. */
	planning_frameworks?: FrameworkInventory[];
}

export interface Project {
	id: Uuid;
	name: string;
	root_path: string;
	git_remote: string | null;
	profile: ProjectProfile | null;
	created_at: Timestamp;
	last_seen_at: Timestamp;
	/** Key prefix for this project's task keys, the `ATL` in `ATL-12`. */
	board_key: string | null;
	/** Stage override; null means the project follows the global list. */
	board_stages: Stage[] | null;
	/** Never null on a read: an unset column comes back as the all-null default. */
	agent_access: AgentAccess;
	/** Null until an override is stored. `api_key` reads back as `"***"`. */
	extraction: ProjectExtraction | null;
	/** MCP tool names disabled for this project on top of the global `mcp.disabled_tools` list. */
	mcp_disabled_tools: string[];
}

/**
 * Who may write memories and move tasks in a project. `null` for either list means any
 * actor. A list matches the whole label (`claude-code/reviewer`) or the part before the
 * slash. `desktop` and anything starting with `cli` are exempt: those are the user's own
 * hands, not an agent.
 */
export interface AgentAccess {
	memory_writers: string[] | null;
	task_movers: string[] | null;
	require_review: boolean;
}

/**
 * A project's extraction override. Every field is optional and an absent one falls back
 * to the matching global `extraction.*` setting, field by field. Sending
 * `api_key: "***"` back means "leave the stored key alone".
 */
export interface ProjectExtraction {
	enabled?: boolean | null;
	base_url?: string | null;
	model?: string | null;
	api_key?: string | null;
	auto_accept_min_confidence?: number | null;
}

/**
 * `PATCH /projects/{id}`. Only the fields present change; `git_remote: null` clears the
 * remote where leaving the field out keeps it, which is the daemon's double option.
 */
export interface ProjectPatch {
	name?: string;
	board_key?: string;
	git_remote?: string | null;
	/** Replaces the project's MCP tool override wholesale when present. */
	mcp_disabled_tools?: string[];
}

/**
 * What a log row points at. `key` is set for tasks only. `run` is in the union because
 * the Log tab links one; the daemon does not emit it yet.
 */
export type LogRefType = 'task' | 'memory' | 'project' | 'sync' | 'job' | 'run';

export interface LogRef {
	type: LogRefType;
	id: string;
	key?: string | null;
}

/** One row of `GET /projects/{id}/log`, newest first. */
export interface LogEntry {
	time: Timestamp;
	source: string;
	kind: string;
	detail: string;
	ref: LogRef | null;
}

/** The log's query string. `after` keeps entries strictly older than the time given. */
export interface LogFilter {
	source?: string | null;
	kind?: string | null;
	q?: string | null;
	after?: Timestamp | null;
	limit?: number | null;
}

export interface ProjectContext {
	project: Project;
	memories: RecallHit[];
	practices: Doc[];
	workflows: WorkflowSummary[];
}

export interface NewAgent {
	name: string;
	description: string;
	instructions: string;
	model_hint?: string | null;
	tools?: string[];
	tags?: string[];
}

export interface Agent {
	id: Uuid;
	name: string;
	description: string;
	instructions: string;
	model_hint: string | null;
	tools: string[];
	tags: string[];
	version: number;
	created_at: Timestamp;
	updated_at: Timestamp;
}

export type DocKind = 'practice' | 'workflow';

export interface NewDoc {
	name: string;
	body: string;
	tags?: string[];
	project_id?: Uuid | null;
}

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

export type SyncKind = 'claude' | 'codex' | 'agents_md' | 'claude_md' | 'framework_instructions';

// `SyncAction` carries no serde rename, so the unit variants serialize as their
// Rust names and `Skip(String)` as serde's externally tagged `{ "Skip": reason }`.
export type SyncAction = 'Create' | 'Update' | 'Unchanged' | { Skip: string };

export interface SyncOp {
	kind: SyncKind;
	path: string;
	content: string;
	action: SyncAction;
}

export interface SyncRequest {
	root?: string | null;
	global?: boolean;
	targets?: SyncKind[];
	check_only?: boolean;
}

export interface SyncReport {
	ops: SyncOp[];
	created: number;
	updated: number;
	unchanged: number;
	skipped: number;
}

/** `GET/PUT /api/v1/settings` is a flat key/value map; `extraction.api_key` reads back as `"***"`. */
export type Settings = Record<string, unknown>;

/** The desktop theme, mirrored into the daemon so a second client agrees. */
export const UI_THEME_KEY = 'ui.theme';

/** An imported theme pack's JSON text, or unset for none. */
export const UI_THEME_PACK_KEY = 'ui.theme_pack';
/** `"system"`, `"inter"` or `"jetbrains-mono"`. */
export const UI_FONT_UI_KEY = 'ui.font_ui';
/** `"jetbrains-mono"` or `"system-mono"`. */
export const UI_FONT_MONO_KEY = 'ui.font_mono';
/** The UI size scale's base step: 11, 12 or 13. */
export const UI_FONT_SIZE_KEY = 'ui.font_size';
/** The whole app's zoom level, an integer percent: 80, 90, 100, 110, 125 or 150. */
export const UI_SCALE_KEY = 'ui.scale';

/**
 * `{ "name", "base": "dark"|"light", "tokens": { "--token": "value" } }`. Overrides
 * colour tokens and the two radius tokens (`--radius-sm`, `--radius-md`) on top of its
 * base theme; the allowed token names and value syntax are validated both by the
 * daemon (`crates/atlas-core/src/settings.rs`) and by `$lib/shell/theme-pack.ts`.
 */
export interface ThemePack {
	name: string;
	base: 'dark' | 'light';
	tokens: Record<string, string>;
}

export type JobStatus = 'queued' | 'running' | 'done' | 'failed';

/** A background extraction job, returned by `POST /ingest` (as `job_id`) and `GET /jobs/{id}`. */
export interface Job {
	id: Uuid;
	kind: string;
	status: JobStatus;
	payload: unknown;
	result: unknown;
	error: string | null;
	created_at: Timestamp;
	updated_at: Timestamp;
}

/** `POST /extraction/test`'s body: 200 carries `reply`, the 400 connectivity failure carries `error`. */
export interface ExtractionTestResult {
	ok: boolean;
	reply?: string;
	error?: string;
}

/** The reason on a skipped op, or null when the action is not a skip. */
export function skipReason(action: SyncAction): string | null {
	return typeof action === 'object' ? action.Skip : null;
}

// ---- board ----

export type TaskKind = 'task' | 'bug' | 'feature' | 'chore';
export type TaskPriority = 'low' | 'medium' | 'high' | 'urgent';

/** One column of the board. `done` stages stamp `closed_at` on the tasks that land in them. */
export interface Stage {
	name: string;
	done: boolean;
}

/** The stages in force for a project, and whether they are the project's own. */
export interface StageList {
	stages: Stage[];
	overridden: boolean;
}

export interface Task {
	id: Uuid;
	/** The human handle, e.g. `ATL-12`. Every route takes this or the id. */
	key: string;
	project_id: Uuid | null;
	seq: number;
	title: string;
	description: string;
	stage: string;
	kind: TaskKind;
	priority: TaskPriority;
	assignee: string | null;
	labels: string[];
	parent_id: Uuid | null;
	created_by: string;
	created_at: Timestamp;
	updated_at: Timestamp;
	closed_at: Timestamp | null;
	/** Where this task was imported from, when it was. Absent for a task nobody
	 * imported. */
	source_ref?: SourceRef | null;
	/** Keys of the tasks this one waits on. */
	blocked_by: string[];
	/**
	 * How many of `blocked_by` are not themselves done. This is the count the ready
	 * rule uses, so a badge drawn from it agrees with `ready`; `blocked_by.length`
	 * counts finished blockers too.
	 */
	open_blockers: number;
	/** Computed on read: open, every blocker done, and no open subtask. */
	ready: boolean;
	blocked_reason: string | null;
}

export interface NewTask {
	project_id?: Uuid | null;
	title: string;
	description?: string;
	kind?: TaskKind;
	priority?: TaskPriority;
	assignee?: string | null;
	labels?: string[];
	/** Parent task, by id or key. */
	parent?: string;
	/** Blocking tasks, by id or key. */
	blocked_by?: string[];
	stage?: string;
	/** Set by an import so a re-import finds this task again. */
	source_ref?: SourceRef | null;
}

/**
 * Only the fields present are changed. `assignee: null` and `parent: null` clear
 * the field; leaving either out keeps what is stored.
 */
export interface TaskUpdate {
	title?: string;
	description?: string;
	kind?: TaskKind;
	priority?: TaskPriority;
	assignee?: string | null;
	labels?: string[];
	parent?: string | null;
	expected_updated_at?: Timestamp;
}

/** One line of a task's history: created, edited, moved, commented and the rest. */
export interface TaskEvent {
	id: Uuid;
	task_id: Uuid;
	actor: string;
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

export interface TaskFilter {
	project_id?: Uuid | null;
	stage?: string | null;
	assignee?: string | null;
	/** Keep only the tasks that are ready to be worked on. */
	ready?: boolean;
	/** Case-insensitive substring match over key, title and description. */
	query?: string | null;
	include_done?: boolean;
}

/** One row of `GET /tasks/counts`; every stage appears, including empty ones. */
export interface StageCount {
	stage: string;
	count: number;
}

// ---- workflows (crates/atlas-core/src/workflow, crates/atlas-core/src/models.rs) ----

export type TriggerKind = 'manual' | 'schedule' | 'prompt';
export type NodeKind = 'trigger' | 'action' | 'output';
/** `success`, not `succeeded`: the exact wire string `str_enum!(RunStatus ...)` writes. */
export type RunStatus = 'queued' | 'running' | 'success' | 'failed' | 'cancelled';
export type StepStatus = 'queued' | 'running' | 'success' | 'failed' | 'skipped' | 'cancelled';
export type LogLevel = 'INFO' | 'WARN' | 'ERR';

/** `cron` is read only when `kind` is `schedule`, `prompt` only when it is `prompt`; both
 * are carried on every trigger so the editor keeps a half-typed value across a kind switch. */
export interface Trigger {
	kind: TriggerKind;
	cron: string | null;
	prompt: string | null;
}

export interface GraphPosition {
	x: number;
	y: number;
}

/** Empty `kinds` or `tags` mean "no filter on that axis", not "match nothing". */
export interface MemorySource {
	kinds: string[];
	tags: string[];
	limit: number;
	project_id: Uuid | null;
}

export interface ActionNodeData {
	name: string;
	instructions: string;
	agent: string;
	practices: string[];
	memories: MemorySource | null;
}

export interface OutputNodeData {
	propose_memories: boolean;
	file_tasks: boolean;
}

/** Untagged on the wire: a trigger node's `data` is a bare `Trigger`, an action node's is
 * `ActionNodeData`, an output node's is `OutputNodeData`. The node's own `kind` says which. */
export type NodeData = Trigger | ActionNodeData | OutputNodeData;

/** Named `WorkflowNode` here because `Node` is `@xyflow/svelte`'s own type. */
export interface WorkflowNode {
	id: string;
	kind: NodeKind;
	position: GraphPosition;
	data: NodeData;
}

export interface WorkflowEdge {
	id: string;
	source: string;
	target: string;
}

export interface Graph {
	nodes: WorkflowNode[];
	edges: WorkflowEdge[];
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

/** A workflow's shape without its graph, the way `workflow_list` (MCP) and
 * `ProjectContext.workflows` answer. */
export interface WorkflowSummary {
	id: Uuid;
	name: string;
	trigger: TriggerKind;
	action_count: number;
	enabled: boolean;
	last_status: RunStatus | null;
}

export interface NewWorkflow {
	name: string;
	project_id?: Uuid | null;
	description?: string;
	trigger: Trigger;
	graph?: Graph;
	enabled?: boolean;
}

/** Only the fields present change; `project_id: null` clears the project (the daemon's
 * double option), leaving it out keeps what is stored. */
export interface WorkflowPatch {
	name?: string;
	project_id?: Uuid | null;
	description?: string;
	trigger?: Trigger;
	graph?: Graph;
	enabled?: boolean;
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

/** `GET /api/v1/runs/{id}`. */
export interface RunDetail {
	run: WorkflowRun;
	steps: WorkflowStep[];
}

/**
 * `WorkflowRun.summary` once parsed: written only once a run finishes successfully
 * (`crates/atlas-core/src/workflow/run.rs`), so a queued, running, failed or cancelled
 * run's `summary` stays `null` and the run detail view falls back to the step list for
 * a step count and `n/a` for tokens.
 */
export interface RunSummary {
	steps: number;
	memories_proposed: number;
	tasks_filed: number;
	trigger: TriggerKind;
	tokens: number | null;
}

// ---- global search (GET /api/v1/search, crates/atlas-core/src/search/global.rs) ----

/** The seven kinds a search result can hold, in the order the daemon returns them. */
export type SearchKind = 'task' | 'memory' | 'project' | 'file' | 'commit' | 'event' | 'workflow';

export interface SearchHit {
	kind: SearchKind;
	id: string;
	title: string;
	subtitle: string | null;
	project_id: Uuid | null;
	/** The hit's human pointer: a task key for tasks, the entity id for events. */
	reference: string | null;
	score: number;
	/** Char offsets `[start, end)` into `title`, empty when the hit scored elsewhere. */
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

// ---- MCP (Phase 10) ----
// `GET /api/v1/mcp/status` (crates/atlasd/src/http.rs: McpStatusReport). `McpResource`
// and `McpPrompt` mirror the rmcp `Resource` and `Prompt` wire shapes (camelCase,
// optional fields omitted rather than null).

export type McpToolScope = 'read' | 'write';

export interface McpToolRow {
	name: string;
	description: string;
	args: string;
	scope: McpToolScope;
	enabled: boolean;
}

export interface McpResource {
	uri: string;
	name: string;
	title?: string;
	description?: string;
	mimeType?: string;
	size?: number;
}

export interface McpPromptArgument {
	name: string;
	title?: string;
	description?: string;
	required?: boolean;
}

export interface McpPrompt {
	name: string;
	title?: string;
	description?: string;
	arguments?: McpPromptArgument[];
}

export type McpTransportKind = 'stdio' | 'http';

export interface McpClient {
	id: string;
	transport: McpTransportKind;
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

export interface McpStatusReport {
	transports: {
		stdio: { command: string };
		http: { url: string; protocol_version: string };
	};
	counts: { tools: number; resources: number; prompts: number; clients: number };
	tools: McpToolRow[];
	resources: McpResource[];
	prompts: McpPrompt[];
	clients: McpClient[];
}

// ---- Project MCP view (Task MCP-A) ----
// `GET /api/v1/projects/{id}/mcp` (crates/atlasd/src/http.rs: ProjectMcpReport). Tool
// gating applies at call time, not at `tools/list`, so this route (not the live tool
// list) is where a project's own MCP overrides show.

export interface ProjectMcpToolRow {
	name: string;
	description: string;
	args: string;
	scope: McpToolScope;
	enabled_globally: boolean;
	/** Actually callable here: enabled globally and not in this project's own override. */
	enabled_here: boolean;
}

export interface ProjectMcpReport {
	tools: ProjectMcpToolRow[];
	/** Only this project's own `atlas://` resources (its context, practices and board). */
	resources: McpResource[];
	prompts: McpPrompt[];
	/** Registry entries whose last call resolved to this project. */
	clients: McpClient[];
	connect: {
		stdio: { command: string };
		http: { url: string; protocol_version: string };
		project_root: string;
	};
}

/** Query string for `GET /api/v1/search`. `project_id` takes one project, not a list. */
export interface GlobalSearchQuery {
	q: string;
	project_id?: Uuid | null;
	kinds?: SearchKind[];
	limit?: number;
}
