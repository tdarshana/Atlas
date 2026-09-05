// The desktop's wire types.
//
// Every type the daemon's Rust models define is generated into ./types.generated.ts from
// the `schemars::JsonSchema` derives (crates/atlas-core/src/tsgen.rs) and re-exported
// from here; `cargo test -p atlas-core` fails when that file is stale, and
// `bun run gen:types` regenerates it. What stays hand-written below is either not a
// Rust model at all (Tauri commands, settings keys, the desktop's own helpers) or a
// daemon-side shape declared in crates/atlasd/src/http.rs without a `JsonSchema`
// derive. A local declaration shadows a generated export of the same name, so when one
// of those moves into models.rs, delete it here.
//
// UUIDs and `DateTime<Utc>` both arrive as strings; the string unions match the
// `str_enum!` literals exactly, so a value that type checks here parses there.

export * from './types.generated';

import type {
	Edge,
	McpToolInfo,
	McpTransport,
	McpTransportInput,
	MemoryScope,
	MemoryScopeFilter,
	Node,
	NodeData,
	Position,
	ProjectAccess,
	SearchQuery,
	SkillUpdate,
	SyncAction,
	Timestamp,
	TriggerKind,
	Uuid,
	WorkflowRun,
	WorkflowStep
} from './types.generated';

// ---- the desktop's names for generated types ----
// Kept so the components read as they did; each is the Rust type under another name.

/**
 * A request-time narrowing, not a value of a memory's own `scope` column (that is
 * `MemoryScope`). `project_only` asks `GET /memories` and `POST /memories/search` for
 * exactly one project's own memories, nothing global; `global_only` narrows to the
 * project-less memories, ignoring any `project_id` sent alongside it; `all` is the
 * default, where a project widens to its own memories plus every global one.
 */
export type MemoryListScope = MemoryScopeFilter;
/** `GET /api/v1/projects/{id}/access`. */
export type ProjectAccessReport = ProjectAccess;
export type GraphPosition = Position;
/** Named `WorkflowNode` here because `Node` is `@xyflow/svelte`'s own type. */
export type WorkflowNode = Node;
export type WorkflowEdge = Edge;
/** The action and output arms of the untagged `NodeData`; a trigger node's `data` is a bare `Trigger`. */
export type ActionNodeData = Extract<NodeData, { agent: string }>;
export type OutputNodeData = Extract<NodeData, { propose_memories: boolean }>;
export type McpServerTransport = McpTransport;
export type McpStdioTransport = Extract<McpTransport, { kind: 'stdio' }>;
export type McpHttpTransport = Extract<McpTransport, { kind: 'http' }>;
/** `POST /api/v1/mcp/servers`'s transport, secret values included. */
export type NewMcpTransport = McpTransportInput;
export type McpCheckTool = McpToolInfo;
/** `PATCH /api/v1/skills/{id}`, native skills only. */
export type SkillPatch = SkillUpdate;
export type SkillScope = MemoryScope;
/** Query string for `GET /api/v1/search`. `project_id` takes one project, not a list. */
export type GlobalSearchQuery = SearchQuery;

// ---- not Rust models ----

/**
 * A window over `GET /memories`: up to `limit` rows after skipping `offset`, in the
 * route's own newest-first order. Neither set is the whole set. The daemon caps
 * `limit` at 1000. Query parameters, not a serialised model.
 */
export interface MemoryPage {
	limit?: number;
	offset?: number;
}

/**
 * What a log row's `ref.type` holds. The daemon types it as a plain string; this is the
 * desktop's own list. `run` is in the union because the Log tab links one; the daemon
 * does not emit it yet.
 */
export type LogRefType = 'task' | 'memory' | 'project' | 'sync' | 'job' | 'run';

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

/** The reason on a skipped op, or null when the action is not a skip. */
export function skipReason(action: SyncAction): string | null {
	return typeof action === 'object' ? action.Skip : null;
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

// ---- daemon shapes without a JsonSchema model (crates/atlas-core/src/jobs.rs, crates/atlasd/src/http.rs) ----

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

/** One row of `GET /tasks/counts`; every stage appears, including empty ones. */
export interface StageCount {
	stage: string;
	count: number;
}

/** `GET /api/v1/runs/{id}`. */
export interface RunDetail {
	run: WorkflowRun;
	steps: WorkflowStep[];
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
	/** `builtin` for a tool the daemon carries itself, `plugin:<id>` for one a plugin
	 * registered. Optional so a report from an older daemon still type-checks. */
	source?: string;
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
	/** Read the same way as `McpToolRow.source`: `builtin`, or `plugin:<id>` for a tool a
	 * plugin contributed. Optional only for a report written by an older daemon. */
	source?: string;
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
