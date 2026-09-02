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
