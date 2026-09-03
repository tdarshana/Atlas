// Typed client for the daemon's JSON API (crates/atlasd/src/http.rs). This is the
// only way the GUI reaches Atlas data: it never opens the DuckDB file and never
// shells out to `atlas`.

import type {
	Agent,
	Doc,
	DocKind,
	ExtractionTestResult,
	Job,
	Memory,
	MemoryStatus,
	NewAgent,
	NewDoc,
	NewMemory,
	NewTask,
	Project,
	ProjectContext,
	RecallHit,
	RecallQuery,
	Settings,
	Stage,
	StageCount,
	StageList,
	StatusReport,
	SyncReport,
	SyncRequest,
	Task,
	TaskDetail,
	TaskEvent,
	TaskFilter,
	TaskUpdate,
	Timestamp,
	Uuid
} from './types';

/** A non-2xx response, carrying the daemon's `error` string as the message. */
export class ApiError extends Error {
	constructor(
		message: string,
		public status: number
	) {
		super(message);
		this.name = 'ApiError';
	}
}

/** The `X-Atlas-Actor` every board request sends, which is how the daemon labels
 * the events it writes for us. */
const BOARD_ACTOR = 'desktop';

/** Plural route segment for a doc kind: practice -> practices. */
const docPath = (kind: DocKind) => (kind === 'practice' ? 'practices' : 'workflows');

function query(params: Record<string, string | null | undefined>): string {
	const q = new URLSearchParams();
	for (const [k, v] of Object.entries(params)) if (v != null && v !== '') q.set(k, v);
	const s = q.toString();
	return s ? `?${s}` : '';
}

export class AtlasApi {
	constructor(public baseUrl: string) {}

	// ---- memories ----

	status(): Promise<StatusReport> {
		return this.req('GET', '/api/v1/status');
	}

	listMemories(status?: MemoryStatus, projectId?: Uuid | null): Promise<Memory[]> {
		return this.req('GET', `/api/v1/memories${query({ status, project_id: projectId })}`);
	}

	search(q: RecallQuery): Promise<RecallHit[]> {
		return this.req('POST', '/api/v1/memories/search', q);
	}

	remember(m: NewMemory): Promise<Memory> {
		return this.req('POST', '/api/v1/memories', m);
	}

	getMemory(id: Uuid): Promise<Memory> {
		return this.req('GET', `/api/v1/memories/${encodeURIComponent(id)}`);
	}

	forget(id: Uuid, reason?: string): Promise<Memory> {
		// An empty body is a forget with no reason; the daemon rejects a body that
		// is not JSON, so only send one when there is a reason to send.
		const body = reason ? { reason } : undefined;
		return this.req('POST', `/api/v1/memories/${encodeURIComponent(id)}/forget`, body);
	}

	setMemoryStatus(id: Uuid, status: MemoryStatus): Promise<Memory> {
		return this.req('POST', `/api/v1/memories/${encodeURIComponent(id)}/status`, { status });
	}

	// ---- projects ----

	listProjects(): Promise<Project[]> {
		return this.req('GET', '/api/v1/projects');
	}

	connectProject(root: string): Promise<Project> {
		return this.req('POST', '/api/v1/projects/connect', { root });
	}

	getProject(id: Uuid): Promise<Project> {
		return this.req('GET', `/api/v1/projects/${encodeURIComponent(id)}`);
	}

	refreshProject(id: Uuid): Promise<Project> {
		return this.req('POST', `/api/v1/projects/${encodeURIComponent(id)}/refresh`);
	}

	projectContext(root: string): Promise<ProjectContext> {
		return this.req('POST', '/api/v1/projects/context', { root });
	}

	/** Removes the project row. Its memories are kept; nothing is hard-deleted. */
	deleteProject(id: Uuid): Promise<void> {
		return this.req('DELETE', `/api/v1/projects/${encodeURIComponent(id)}`);
	}

	// ---- agents ----

	listAgents(): Promise<Agent[]> {
		return this.req('GET', '/api/v1/agents');
	}

	getAgent(name: string): Promise<Agent> {
		return this.req('GET', `/api/v1/agents/${encodeURIComponent(name)}`);
	}

	saveAgent(a: NewAgent): Promise<Agent> {
		return this.req('POST', '/api/v1/agents', a);
	}

	deleteAgent(name: string): Promise<void> {
		return this.req('DELETE', `/api/v1/agents/${encodeURIComponent(name)}`);
	}

	// ---- practices and workflows ----

	listDocs(kind: DocKind, projectId?: Uuid | null): Promise<Doc[]> {
		return this.req('GET', `/api/v1/${docPath(kind)}${query({ project_id: projectId })}`);
	}

	getDoc(kind: DocKind, name: string): Promise<Doc> {
		return this.req('GET', `/api/v1/${docPath(kind)}/${encodeURIComponent(name)}`);
	}

	saveDoc(kind: DocKind, d: NewDoc): Promise<Doc> {
		return this.req('POST', `/api/v1/${docPath(kind)}`, d);
	}

	deleteDoc(kind: DocKind, name: string): Promise<void> {
		return this.req('DELETE', `/api/v1/${docPath(kind)}/${encodeURIComponent(name)}`);
	}

	// ---- sync and settings ----

	sync(req: SyncRequest): Promise<SyncReport> {
		return this.req('POST', '/api/v1/sync', req);
	}

	getSettings(): Promise<Settings> {
		return this.req('GET', '/api/v1/settings');
	}

	setSettings(partial: Settings): Promise<Settings> {
		return this.req('PUT', '/api/v1/settings', partial);
	}

	// ---- extraction ----

	/**
	 * Queues a transcript for extraction. 202 carries the job id to follow with
	 * `getJob`; a disabled or half-configured setup throws `ApiError` with status
	 * 409 and the daemon's "extraction is disabled" message.
	 */
	ingest(text: string, sourceTool: string, projectRoot?: string): Promise<{ job_id: Uuid }> {
		return this.req('POST', '/api/v1/ingest', {
			text,
			source_tool: sourceTool,
			project_root: projectRoot
		});
	}

	getJob(id: Uuid): Promise<Job> {
		return this.req('GET', `/api/v1/jobs/${encodeURIComponent(id)}`);
	}

	/**
	 * A connectivity check against the configured model. Unlike every other route,
	 * a model error comes back as 400 with `{ok: false, error}` rather than the
	 * usual `{error}` shape, so it is returned as data instead of thrown; a 409
	 * (extraction disabled) still throws `ApiError` like any other route.
	 */
	async testExtraction(): Promise<ExtractionTestResult> {
		let res: Response;
		try {
			res = await fetch(`${this.baseUrl}/api/v1/extraction/test`, {
				method: 'POST',
				headers: { Accept: 'application/json' }
			});
		} catch (e) {
			throw new ApiError(e instanceof Error ? e.message : String(e), 0);
		}
		const text = await res.text();
		if (res.status === 400) return JSON.parse(text) as ExtractionTestResult;
		if (!res.ok) throw new ApiError(errorMessage(text, res), res.status);
		return JSON.parse(text) as ExtractionTestResult;
	}

	// ---- board ----

	listTasks(filter: TaskFilter = {}): Promise<Task[]> {
		return this.boardReq(
			'GET',
			`/api/v1/tasks${query({
				project_id: filter.project_id,
				stage: filter.stage,
				assignee: filter.assignee,
				ready: filter.ready ? 'true' : null,
				q: filter.query,
				include_done: filter.include_done ? 'true' : null
			})}`
		);
	}

	/** The task plus its subtasks and its history. Takes a key or an id. */
	getTask(key: string): Promise<TaskDetail> {
		return this.boardReq('GET', `/api/v1/tasks/${encodeURIComponent(key)}`);
	}

	createTask(body: NewTask): Promise<Task> {
		return this.boardReq('POST', '/api/v1/tasks', body);
	}

	/**
	 * Only the fields in `body` change. `assignee: null` clears the assignee; leaving
	 * the field out keeps it, which is what the daemon's double option means.
	 */
	updateTask(key: string, body: TaskUpdate): Promise<Task> {
		return this.boardReq('PATCH', `/api/v1/tasks/${encodeURIComponent(key)}`, body);
	}

	/** `expectedUpdatedAt` turns a concurrent edit into a 409 instead of a clobber. */
	moveTask(key: string, stage: string, expectedUpdatedAt?: Timestamp): Promise<Task> {
		return this.boardReq('POST', `/api/v1/tasks/${encodeURIComponent(key)}/move`, {
			stage,
			expected_updated_at: expectedUpdatedAt
		});
	}

	commentTask(key: string, body: string): Promise<TaskEvent> {
		return this.boardReq('POST', `/api/v1/tasks/${encodeURIComponent(key)}/comment`, { body });
	}

	/** Assigns the task to this app's actor. `force` takes it from someone else. */
	claimTask(key: string, force = false): Promise<Task> {
		return this.boardReq('POST', `/api/v1/tasks/${encodeURIComponent(key)}/claim`, { force });
	}

	/** Replaces the blocker list wholesale; an empty array clears it. */
	setTaskBlockers(key: string, keys: string[]): Promise<Task> {
		return this.boardReq('PUT', `/api/v1/tasks/${encodeURIComponent(key)}/blockers`, {
			blocked_by: keys
		});
	}

	deleteTask(key: string): Promise<void> {
		return this.boardReq('DELETE', `/api/v1/tasks/${encodeURIComponent(key)}`);
	}

	/** The stages in force: the project's own when it has some, the global list otherwise. */
	boardStages(projectId?: Uuid | null): Promise<StageList> {
		return this.boardReq('GET', `/api/v1/board/stages${query({ project_id: projectId })}`);
	}

	/** `renames` maps an old stage name to its new one, so the tasks in it follow. */
	setBoardStages(stages: Stage[], renames?: Record<string, string>): Promise<Stage[]> {
		return this.boardReq('PUT', '/api/v1/board/stages', { stages, renames });
	}

	/** `stages: null` drops the override and puts the project back on the global list. */
	setProjectStages(
		projectId: Uuid,
		stages: Stage[] | null,
		renames?: Record<string, string>
	): Promise<StageList> {
		return this.boardReq('PUT', `/api/v1/projects/${encodeURIComponent(projectId)}/stages`, {
			stages,
			renames
		});
	}

	taskCounts(projectId?: Uuid | null): Promise<StageCount[]> {
		return this.boardReq('GET', `/api/v1/tasks/counts${query({ project_id: projectId })}`);
	}

	/**
	 * Board writes are recorded against whoever made them, so every board request
	 * carries this app's actor label. The reads send it too; it is ignored there.
	 */
	private boardReq<T>(method: string, path: string, body?: unknown): Promise<T> {
		return this.req(method, path, body, { 'X-Atlas-Actor': BOARD_ACTOR });
	}

	// ---- transport ----

	private async req<T>(
		method: string,
		path: string,
		body?: unknown,
		extra?: Record<string, string>
	): Promise<T> {
		const headers: Record<string, string> = { Accept: 'application/json', ...extra };
		if (body !== undefined) headers['Content-Type'] = 'application/json';

		let res: Response;
		try {
			res = await fetch(`${this.baseUrl}${path}`, {
				method,
				headers,
				body: body === undefined ? undefined : JSON.stringify(body)
			});
		} catch (e) {
			// A transport failure has no status; 0 tells callers to offer the log path.
			throw new ApiError(e instanceof Error ? e.message : String(e), 0);
		}

		const text = await res.text();
		if (!res.ok) throw new ApiError(errorMessage(text, res), res.status);
		if (res.status === 204 || text === '') return undefined as T;
		return JSON.parse(text) as T;
	}
}

/** The daemon's `error` field, falling back to the raw body or the status line. */
function errorMessage(text: string, res: Response): string {
	try {
		const parsed = JSON.parse(text);
		if (parsed && typeof parsed.error === 'string') return parsed.error;
	} catch {
		// Not JSON; fall through to the body text.
	}
	return text || `${res.status} ${res.statusText}`;
}
