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

export interface RecallQuery {
	query: string;
	limit?: number;
	scope?: MemoryScope | null;
	project_id?: Uuid | null;
	kinds?: MemoryKind[];
	tags?: string[];
}

export interface StatusReport {
	version: string;
	db_path: string;
	memories_active: number;
	embedding: string;
	port: number | null;
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
}

export interface ProjectContext {
	project: Project;
	memories: RecallHit[];
	practices: Doc[];
	workflows: Doc[];
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

export type SyncKind = 'claude' | 'codex' | 'agents_md' | 'claude_md';

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
