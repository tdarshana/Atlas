// Typed client for the daemon's JSON API (crates/atlasd/src/http.rs). This is the
// only way the GUI reaches Atlas data: it never opens the DuckDB file and never
// shells out to `atlas`.

import type {
	Agent,
	AgentAccess,
	Doc,
	DocKind,
	ExtractionTestResult,
	FrameworkKind,
	FrameworkListing,
	GlobalSearchQuery,
	ImportReport,
	ImportWhat,
	Job,
	LogEntry,
	LogFilter,
	McpCheckResult,
	McpServerEntry,
	McpServerList,
	McpStatusReport,
	Memory,
	MemoryFacets,
	MemoryListScope,
	MemoryPage,
	MemoryStatus,
	NewAgent,
	NewDoc,
	NewMcpServer,
	NewMemory,
	NewPersona,
	NewSkill,
	NewTask,
	NewWorkflow,
	Persona,
	PersonaBundle,
	PersonaPatch,
	Project,
	ProjectAccessReport,
	ProjectExtraction,
	ProjectMcpReport,
	ProjectPatch,
	ProjectContext,
	RecallHit,
	RecallQuery,
	RosterEntry,
	RosterRow,
	SearchResult,
	Settings,
	Skill,
	SkillList,
	SkillPatch,
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
	TriggerKind,
	RunDetail,
	Uuid,
	Workflow,
	WorkflowPatch,
	WorkflowRun
} from './types';

export type { MemoryListScope } from '$lib/types';

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

	/**
	 * `scope: 'project_only'` (paired with `projectId`) narrows to that project's own
	 * memories, dropping the global ones a bare `project_id` still widens in. `page`
	 * windows the newest-first list; without it the whole set comes back.
	 */
	listMemories(
		status?: MemoryStatus,
		projectId?: Uuid | null,
		scope?: MemoryListScope,
		page?: MemoryPage
	): Promise<Memory[]> {
		return this.req(
			'GET',
			`/api/v1/memories${query({
				status,
				project_id: projectId,
				scope,
				limit: page?.limit?.toString(),
				offset: page?.offset?.toString()
			})}`
		);
	}

	/**
	 * Kind and tag counts, plus the total, over active memories, `project_id`/`scope`
	 * read the same way `listMemories` reads them. Lets a side panel show counts
	 * without loading every matching memory first.
	 */
	memoryFacets(projectId?: Uuid | null, scope?: MemoryListScope | null): Promise<MemoryFacets> {
		return this.req('GET', `/api/v1/memories/facets${query({ project_id: projectId, scope })}`);
	}

	/**
	 * `list_scope` is the same narrowing `listMemories` takes; `scope` stays the memory's
	 * own scope, and the daemon reads the two independently.
	 */
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

	// ---- global search ----

	/**
	 * Cross-entity search: tasks, memories, projects, files, commits, events and
	 * workflows in one call. Named `globalSearch` because `search` above is already
	 * the memories recall route.
	 */
	globalSearch(q: GlobalSearchQuery): Promise<SearchResult> {
		return this.req(
			'GET',
			`/api/v1/search${query({
				q: q.q,
				project_id: q.project_id,
				kinds: q.kinds?.join(','),
				limit: q.limit == null ? null : String(q.limit)
			})}`
		);
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

	/**
	 * Only the fields in `patch` change. `git_remote: null` clears the remote; leaving the
	 * field out keeps it. A new `board_key` renames every task key in the project, so ask
	 * before sending one.
	 */
	patchProject(id: Uuid, patch: ProjectPatch): Promise<Project> {
		return this.req('PATCH', `/api/v1/projects/${encodeURIComponent(id)}`, patch);
	}

	/** Replaces the project's access rules wholesale; a null list means any actor. */
	setAgentAccess(id: Uuid, access: AgentAccess): Promise<Project> {
		return this.req('PUT', `/api/v1/projects/${encodeURIComponent(id)}/agent-access`, access);
	}

	/** The project's rules, the global defaults and what they resolve to. */
	projectAccess(id: Uuid): Promise<ProjectAccessReport> {
		return this.req('GET', `/api/v1/projects/${encodeURIComponent(id)}/access`);
	}

	/** `null` drops the override and puts the project back on the global settings. */
	setProjectExtraction(id: Uuid, extraction: ProjectExtraction | null): Promise<Project> {
		return this.req('PUT', `/api/v1/projects/${encodeURIComponent(id)}/extraction`, extraction);
	}

	/** One page of the project's event history, newest first. */
	projectLog(id: Uuid, filter: LogFilter = {}): Promise<LogEntry[]> {
		return this.req(
			'GET',
			`/api/v1/projects/${encodeURIComponent(id)}/log${query({
				source: filter.source,
				kind: filter.kind,
				q: filter.q,
				after: filter.after,
				limit: filter.limit == null ? null : String(filter.limit)
			})}`
		);
	}

	/** The whole log as JSONL, no filter and no cap. Returned as text, not parsed. */
	projectLogExport(id: Uuid): Promise<string> {
		return this.text('GET', `/api/v1/projects/${encodeURIComponent(id)}/log/export`);
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

	// ---- workflows (graph editor; crates/atlasd Task 2) ----

	listWorkflows(projectId?: Uuid | null): Promise<Workflow[]> {
		return this.workflowReq('GET', `/api/v1/workflows${query({ project_id: projectId })}`);
	}

	getWorkflow(id: Uuid): Promise<Workflow> {
		return this.workflowReq('GET', `/api/v1/workflows/${encodeURIComponent(id)}`);
	}

	createWorkflow(w: NewWorkflow): Promise<Workflow> {
		return this.workflowReq('POST', '/api/v1/workflows', w);
	}

	/** Only the fields in `patch` change. */
	patchWorkflow(id: Uuid, patch: WorkflowPatch): Promise<Workflow> {
		return this.workflowReq('PATCH', `/api/v1/workflows/${encodeURIComponent(id)}`, patch);
	}

	deleteWorkflow(id: Uuid): Promise<void> {
		return this.workflowReq('DELETE', `/api/v1/workflows/${encodeURIComponent(id)}`);
	}

	/** 202: the run is queued, not finished. */
	runWorkflow(id: Uuid, trigger?: TriggerKind, input?: unknown): Promise<WorkflowRun> {
		return this.workflowReq('POST', `/api/v1/workflows/${encodeURIComponent(id)}/run`, {
			trigger,
			input
		});
	}

	listRuns(workflowId: Uuid, limit?: number): Promise<WorkflowRun[]> {
		return this.workflowReq(
			'GET',
			`/api/v1/workflows/${encodeURIComponent(workflowId)}/runs${query({
				limit: limit == null ? null : String(limit)
			})}`
		);
	}

	getRun(id: Uuid): Promise<RunDetail> {
		return this.workflowReq('GET', `/api/v1/runs/${encodeURIComponent(id)}`);
	}

	cancelRun(id: Uuid): Promise<WorkflowRun> {
		return this.workflowReq('POST', `/api/v1/runs/${encodeURIComponent(id)}/cancel`);
	}

	/** The run's log as JSONL, not parsed, like `projectLogExport`. */
	runExport(id: Uuid): Promise<string> {
		return this.text('GET', `/api/v1/runs/${encodeURIComponent(id)}/export`);
	}

	/**
	 * Workflow writes are recorded against whoever made them, so every workflow request
	 * carries this app's actor label, reads included; the daemon ignores it on a read.
	 */
	private workflowReq<T>(method: string, path: string, body?: unknown): Promise<T> {
		return this.req(method, path, body, { 'X-Atlas-Actor': BOARD_ACTOR });
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

	// ---- MCP ----

	/** Transports, counts, the tools table, resources, prompts and connected clients. */
	mcpStatus(): Promise<McpStatusReport> {
		return this.req('GET', '/api/v1/mcp/status');
	}

	/**
	 * What MCP looks like from one project's point of view: its tools table with
	 * `enabled_globally`/`enabled_here`, only this project's own resources, its
	 * prompts, the clients whose last call resolved here, and its connect info.
	 * Tool gating applies at call time, not at the live tool list, so this is where a
	 * project's own overrides show.
	 */
	projectMcp(id: Uuid): Promise<ProjectMcpReport> {
		return this.req('GET', `/api/v1/projects/${encodeURIComponent(id)}/mcp`);
	}

	/** Replaces the project's MCP tool override wholesale; an empty list clears it. */
	setProjectMcpTools(id: Uuid, disabled: string[]): Promise<Project> {
		return this.req('PUT', `/api/v1/projects/${encodeURIComponent(id)}/mcp/tools`, { disabled });
	}

	// ---- MCP servers ----

	/**
	 * Every MCP server the user's agents are wired to. With no project that is the
	 * user-scope entries of each agent; with one it is that project's own scopes plus
	 * the plugin servers and Atlas, which apply everywhere.
	 */
	listMcpServers(projectId?: Uuid | null): Promise<McpServerList> {
		return this.req('GET', `/api/v1/mcp/servers${query({ project_id: projectId })}`);
	}

	/** Starts the server the way its agent would and asks it for its tools. */
	checkMcpServer(id: string, projectId?: Uuid | null): Promise<McpCheckResult> {
		return this.req(
			'POST',
			`/api/v1/mcp/servers/${encodeURIComponent(id)}/check${query({ project_id: projectId })}`
		);
	}

	/** Flips the agent's own switch for one server; only offered where it has one. */
	setMcpServerEnabled(id: string, enabled: boolean, projectId?: Uuid | null): Promise<void> {
		return this.req(
			'PUT',
			`/api/v1/mcp/servers/${encodeURIComponent(id)}/enabled${query({ project_id: projectId })}`,
			{ enabled }
		);
	}

	/** Writes a new entry into the agent's own config file. */
	addMcpServer(input: NewMcpServer): Promise<McpServerEntry> {
		return this.req('POST', '/api/v1/mcp/servers', input);
	}

	/** Deletes the entry from the config file it came from. */
	removeMcpServer(id: string, projectId?: Uuid | null): Promise<void> {
		return this.req(
			'DELETE',
			`/api/v1/mcp/servers/${encodeURIComponent(id)}${query({ project_id: projectId })}`
		);
	}

	// ---- skills ----

	/**
	 * Every skill that applies: the Atlas-native ones plus the `SKILL.md` folders the
	 * daemon discovers. With `projectId` the project's own roots are searched too and
	 * each row carries `enabled_here`.
	 */
	listSkills(projectId?: Uuid | null): Promise<SkillList> {
		return this.req('GET', `/api/v1/skills${query({ project_id: projectId })}`);
	}

	/** One skill with its body and the other files in its folder. */
	getSkill(id: string, projectId?: Uuid | null): Promise<Skill> {
		return this.req(
			'GET',
			`/api/v1/skills/${encodeURIComponent(id)}${query({ project_id: projectId })}`
		);
	}

	/** Creates an Atlas-native skill, global or scoped to one project. */
	createSkill(input: NewSkill): Promise<Skill> {
		return this.req('POST', '/api/v1/skills', input);
	}

	/**
	 * Edit in place: rewrites a native skill's body, or the `SKILL.md` on disk. A
	 * project-scoped discovered skill is only reachable with `projectId`, since the
	 * daemon resolves the id against a listing of that project's roots taken right now.
	 */
	updateSkillBody(id: string, body: string, projectId?: Uuid | null): Promise<Skill> {
		return this.req(
			'PUT',
			`/api/v1/skills/${encodeURIComponent(id)}${query({ project_id: projectId })}`,
			{ body }
		);
	}

	/** Name and description of a native skill. Native ids are bare UUIDs, so this route
	 * takes no `project_id`. */
	patchSkill(id: string, patch: SkillPatch): Promise<Skill> {
		return this.req('PATCH', `/api/v1/skills/${encodeURIComponent(id)}`, patch);
	}

	/** Native skills only; a discovered one belongs to the folder it came from. Native ids
	 * are bare UUIDs, so this route takes no `project_id` either. */
	deleteSkill(id: string): Promise<void> {
		return this.req('DELETE', `/api/v1/skills/${encodeURIComponent(id)}`);
	}

	/** Replaces the project's disabled skill list wholesale; an empty list clears it. */
	setProjectSkills(id: Uuid, disabled: string[]): Promise<Project> {
		return this.req('PUT', `/api/v1/projects/${encodeURIComponent(id)}/skills`, { disabled });
	}

	// ---- personas ----

	listPersonas(): Promise<Persona[]> {
		return this.req('GET', '/api/v1/personas');
	}

	/** Takes an id, a slug or a name. */
	getPersona(idOrSlug: string): Promise<Persona> {
		return this.req('GET', `/api/v1/personas/${encodeURIComponent(idOrSlug)}`);
	}

	/** Every persona write is audited against this app's actor, like a board write. */
	createPersona(input: NewPersona): Promise<Persona> {
		return this.boardReq('POST', '/api/v1/personas', input);
	}

	/** Only the fields in `patch` change. Takes the id, not the slug. */
	updatePersona(id: Uuid, patch: PersonaPatch): Promise<Persona> {
		return this.boardReq('PUT', `/api/v1/personas/${encodeURIComponent(id)}`, patch);
	}

	deletePersona(id: Uuid): Promise<void> {
		return this.boardReq('DELETE', `/api/v1/personas/${encodeURIComponent(id)}`);
	}

	/** The persona with everything it references resolved; missing references are warnings. */
	getPersonaBundle(id: string, projectId?: Uuid | null): Promise<PersonaBundle> {
		return this.req(
			'GET',
			`/api/v1/personas/${encodeURIComponent(id)}/bundle${query({ project_id: projectId })}`
		);
	}

	getProjectRoster(projectId: Uuid): Promise<RosterRow[]> {
		return this.req('GET', `/api/v1/projects/${encodeURIComponent(projectId)}/personas`);
	}

	/** Replaces the project's roster wholesale: ids, the default and the order. */
	setProjectRoster(projectId: Uuid, entries: RosterEntry[]): Promise<RosterRow[]> {
		return this.boardReq('PUT', `/api/v1/projects/${encodeURIComponent(projectId)}/personas`, entries);
	}

	// ---- extraction ----

	/**
	 * Queues a transcript for extraction. 202 carries the job id to follow with
	 * `getJob`; a disabled or half-configured setup throws `ApiError` with status
	 * 409 and the daemon's "extraction is disabled" message.
	 */
	ingest(text: string, sourceTool: string, projectRoot?: string): Promise<{ job_id: Uuid }> {
		return this.req(
			'POST',
			'/api/v1/ingest',
			{ text, project_root: projectRoot },
			{ 'X-Atlas-Actor': sourceTool }
		);
	}

	getJob(id: Uuid): Promise<Job> {
		return this.req('GET', `/api/v1/jobs/${encodeURIComponent(id)}`);
	}

	/**
	 * A connectivity check against the configured model. Unlike every other route,
	 * a model error comes back as 400 with `{ok: false, error}` rather than the
	 * usual `{error}` shape, so it is returned as data instead of thrown; a 409
	 * (extraction disabled) still throws `ApiError` like any other route.
	 *
	 * `projectId` tests the settings that project resolves to, override and all.
	 */
	async testExtraction(projectId?: Uuid | null): Promise<ExtractionTestResult> {
		let res: Response;
		try {
			res = await fetch(`${this.baseUrl}/api/v1/extraction/test${query({ project_id: projectId })}`, {
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
				persona: filter.persona,
				ready: filter.ready ? 'true' : null,
				q: filter.query,
				include_done: filter.include_done ? 'true' : null,
				top_level: filter.top_level === undefined ? null : filter.top_level ? 'true' : 'false'
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

	taskCounts(projectId?: Uuid | null, topLevel?: boolean): Promise<StageCount[]> {
		return this.boardReq(
			'GET',
			`/api/v1/tasks/counts${query({
				project_id: projectId,
				top_level: topLevel === undefined ? null : topLevel ? 'true' : 'false'
			})}`
		);
	}

	/**
	 * Board writes are recorded against whoever made them, so every board request
	 * carries this app's actor label. The reads send it too; it is ignored there.
	 */
	private boardReq<T>(method: string, path: string, body?: unknown): Promise<T> {
		return this.req(method, path, body, { 'X-Atlas-Actor': BOARD_ACTOR });
	}

	// ---- frameworks ----

	/** Every framework detected in the project, each with the documents it holds. */
	listFrameworks(projectId: Uuid): Promise<FrameworkListing[]> {
		return this.req('GET', `/api/v1/projects/${encodeURIComponent(projectId)}/frameworks`);
	}

	/**
	 * One document's text, `path` as `listFrameworks` gave it back. Each `/`-separated
	 * component is percent-encoded on its own so a slash in `path` stays a path
	 * separator rather than becoming part of one segment.
	 */
	async getFrameworkDoc(projectId: Uuid, kind: FrameworkKind, path: string): Promise<string> {
		const encodedPath = path.split('/').map(encodeURIComponent).join('/');
		const { content } = await this.req<{ content: string }>(
			'GET',
			`/api/v1/projects/${encodeURIComponent(projectId)}/frameworks/${kind}/docs/${encodedPath}`
		);
		return content;
	}

	/**
	 * Imports the framework's tasks onto the board, or its decisions as pending
	 * memories. A write, recorded under this app's actor like every board request.
	 */
	importFramework(projectId: Uuid, kind: FrameworkKind, what: ImportWhat): Promise<ImportReport> {
		return this.boardReq(
			'POST',
			`/api/v1/projects/${encodeURIComponent(projectId)}/frameworks/${kind}/import`,
			{ what }
		);
	}

	// ---- transport ----

	/**
	 * A route whose body is not JSON. The log export is JSONL, which `JSON.parse` would
	 * choke on, so it comes back as the text it is.
	 */
	private async text(method: string, path: string): Promise<string> {
		let res: Response;
		try {
			res = await fetch(`${this.baseUrl}${path}`, { method });
		} catch (e) {
			throw new ApiError(e instanceof Error ? e.message : String(e), 0);
		}
		const body = await res.text();
		if (!res.ok) throw new ApiError(errorMessage(body, res), res.status);
		return body;
	}

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
