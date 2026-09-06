// Generated from crates/atlasd/src/routes.rs by crates/atlasd/src/apigen.rs. Do not edit.
// Regenerate with: ATLAS_WRITE_TS=1 cargo test -p atlasd --bin atlasd apigen
//
// One method per route, on top of the transport `./api`'s `AtlasApi` supplies. The
// routes the table marks hand-written stay in `./api`.

import type {
	AgentAccess,
	BlockersBody,
	ClaimBody,
	CommentBody,
	ForgetBody,
	FrameworkImportBody,
	FrameworkKind,
	FrameworkListing,
	GlobalSearchQuery,
	ImportReport,
	ImportWhat,
	Job,
	LogEntry,
	LogFilter,
	McpCheckResult,
	McpClient,
	McpEnabledBody,
	McpHeartbeatBody,
	McpServerEntry,
	McpServerList,
	McpStatusReport,
	McpToolsBody,
	Memory,
	MemoryFacets,
	MemoryListScope,
	MemoryPage,
	MemoryStatus,
	MoveBody,
	NewMcpServer,
	NewMemory,
	NewPersona,
	NewSkill,
	NewTask,
	NewWorkflow,
	Persona,
	PersonaBundle,
	PersonaPatch,
	PluginToolCallBody,
	PluginToolDecl,
	PluginToolsBody,
	Project,
	ProjectAccessReport,
	ProjectContext,
	ProjectExtraction,
	ProjectMcpReport,
	ProjectPatch,
	RecallHit,
	RecallQuery,
	RegisterMcpClientBody,
	RootBody,
	RosterEntry,
	RosterRow,
	RunDetail,
	SearchResult,
	SetProjectStagesBody,
	SetStagesBody,
	Settings,
	Skill,
	SkillBodyBody,
	SkillList,
	SkillPatch,
	SkillsDisabledBody,
	Stage,
	StageCount,
	StageList,
	StatusBody,
	StatusReport,
	SyncReport,
	SyncRequest,
	Task,
	TaskDetail,
	TaskEvent,
	TaskFilter,
	TaskUpdate,
	Timestamp,
	Uuid,
	Workflow,
	WorkflowPatch,
	WorkflowRun,
} from './types';

/** The `X-Atlas-Actor` the daemon records for every write the app makes; it is
 * ignored on a read. */
const ACTOR = { 'X-Atlas-Actor': 'desktop' };

/**
 * The query string for `params`: a null, undefined or empty value is left out, a
 * list is comma-joined, and anything else is stringified.
 */
export function query(params: Record<string, unknown>): string {
	const q = new URLSearchParams();
	for (const [k, v] of Object.entries(params)) {
		if (v == null) continue;
		const s = Array.isArray(v) ? v.join(',') : String(v);
		if (s !== '') q.set(k, s);
	}
	const s = q.toString();
	return s ? `?${s}` : '';
}

export abstract class GeneratedApi {
	/** The transport: JSON in and out, a non-2xx thrown as `ApiError`. */
	protected abstract req<T>(method: string, path: string, body?: unknown, extra?: Record<string, string>): Promise<T>;
	/** The same, for a route whose answer is not JSON. */
	protected abstract text(method: string, path: string): Promise<string>;

	status(): Promise<StatusReport> {
		return this.req('GET', '/api/v1/status');
	}

	remember(m: NewMemory): Promise<Memory> {
		return this.req('POST', '/api/v1/memories', m);
	}

	/**
	 * `scope: 'project_only'` (paired with `projectId`) narrows to that project's own
	 * memories, dropping the global ones a bare `project_id` still widens in. `page`
	 * windows the newest-first list; without it the whole set comes back.
	 */
	listMemories(status?: MemoryStatus, projectId?: Uuid | null, scope?: MemoryListScope, page: MemoryPage = {}): Promise<Memory[]> {
		return this.req('GET', `/api/v1/memories${query({ status, project_id: projectId, scope, limit: page.limit, offset: page.offset })}`);
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

	getMemory(id: Uuid): Promise<Memory> {
		return this.req('GET', `/api/v1/memories/${encodeURIComponent(id)}`);
	}

	forget(id: Uuid, reason?: string): Promise<Memory> {
		const payload: ForgetBody = { reason };
		return this.req('POST', `/api/v1/memories/${encodeURIComponent(id)}/forget`, payload);
	}

	setMemoryStatus(id: Uuid, status: MemoryStatus): Promise<Memory> {
		const payload: StatusBody = { status };
		return this.req('POST', `/api/v1/memories/${encodeURIComponent(id)}/status`, payload);
	}

	listProjects(): Promise<Project[]> {
		return this.req('GET', '/api/v1/projects');
	}

	connectProject(root: string): Promise<Project> {
		const payload: RootBody = { root };
		return this.req('POST', '/api/v1/projects/connect', payload);
	}

	projectContext(root: string): Promise<ProjectContext> {
		const payload: RootBody = { root };
		return this.req('POST', '/api/v1/projects/context', payload);
	}

	getProject(id: Uuid): Promise<Project> {
		return this.req('GET', `/api/v1/projects/${encodeURIComponent(id)}`);
	}

	/**
	 * Only the fields in `patch` change. `git_remote: null` clears the remote; leaving the
	 * field out keeps it. A new `board_key` renames every task key in the project, so ask
	 * before sending one.
	 */
	patchProject(id: Uuid, patch: ProjectPatch): Promise<Project> {
		return this.req('PATCH', `/api/v1/projects/${encodeURIComponent(id)}`, patch);
	}

	/** Removes the project row. Its memories are kept; nothing is hard-deleted. */
	deleteProject(id: Uuid): Promise<void> {
		return this.req('DELETE', `/api/v1/projects/${encodeURIComponent(id)}`);
	}

	refreshProject(id: Uuid): Promise<Project> {
		return this.req('POST', `/api/v1/projects/${encodeURIComponent(id)}/refresh`);
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
		return this.req('GET', `/api/v1/projects/${encodeURIComponent(id)}/log${query({ source: filter.source, kind: filter.kind, q: filter.q, after: filter.after, limit: filter.limit })}`);
	}

	/** The whole log as JSONL, no filter and no cap. Returned as text, not parsed. */
	projectLogExport(id: Uuid): Promise<string> {
		return this.text('GET', `/api/v1/projects/${encodeURIComponent(id)}/log/export`);
	}

	listWorkflows(projectId?: Uuid | null): Promise<Workflow[]> {
		return this.req('GET', `/api/v1/workflows${query({ project_id: projectId })}`, undefined, ACTOR);
	}

	createWorkflow(w: NewWorkflow): Promise<Workflow> {
		return this.req('POST', '/api/v1/workflows', w, ACTOR);
	}

	getWorkflow(id: Uuid): Promise<Workflow> {
		return this.req('GET', `/api/v1/workflows/${encodeURIComponent(id)}`, undefined, ACTOR);
	}

	/** Only the fields in `patch` change. */
	patchWorkflow(id: Uuid, patch: WorkflowPatch): Promise<Workflow> {
		return this.req('PATCH', `/api/v1/workflows/${encodeURIComponent(id)}`, patch, ACTOR);
	}

	deleteWorkflow(id: Uuid): Promise<void> {
		return this.req('DELETE', `/api/v1/workflows/${encodeURIComponent(id)}`, undefined, ACTOR);
	}

	listRuns(workflowId: Uuid, limit?: number): Promise<WorkflowRun[]> {
		return this.req('GET', `/api/v1/workflows/${encodeURIComponent(workflowId)}/runs${query({ limit })}`, undefined, ACTOR);
	}

	/** Every run across every workflow that finished after `since`, newest first. */
	listAllRuns(since?: Timestamp, limit?: number): Promise<WorkflowRun[]> {
		return this.req('GET', `/api/v1/runs${query({ since, limit })}`);
	}

	getRun(id: Uuid): Promise<RunDetail> {
		return this.req('GET', `/api/v1/runs/${encodeURIComponent(id)}`, undefined, ACTOR);
	}

	cancelRun(id: Uuid): Promise<WorkflowRun> {
		return this.req('POST', `/api/v1/runs/${encodeURIComponent(id)}/cancel`, undefined, ACTOR);
	}

	/** The run's log as JSONL, not parsed, like `projectLogExport`. */
	runExport(id: Uuid): Promise<string> {
		return this.text('GET', `/api/v1/runs/${encodeURIComponent(id)}/export`);
	}

	sync(req: SyncRequest): Promise<SyncReport> {
		return this.req('POST', '/api/v1/sync', req);
	}

	getSettings(): Promise<Settings> {
		return this.req('GET', '/api/v1/settings');
	}

	setSettings(partial: Settings): Promise<Settings> {
		return this.req('PUT', '/api/v1/settings', partial);
	}

	getJob(id: Uuid): Promise<Job> {
		return this.req('GET', `/api/v1/jobs/${encodeURIComponent(id)}`);
	}

	listTasks(filter: TaskFilter = {}): Promise<Task[]> {
		return this.req('GET', `/api/v1/tasks${query({ project_id: filter.project_id, stage: filter.stage, assignee: filter.assignee, persona: filter.persona, ready: filter.ready, q: filter.query, include_done: filter.include_done, top_level: filter.top_level, brief: filter.brief })}`, undefined, ACTOR);
	}

	createTask(body: NewTask): Promise<Task> {
		return this.req('POST', '/api/v1/tasks', body, ACTOR);
	}

	taskCounts(projectId?: Uuid | null, topLevel?: boolean): Promise<StageCount[]> {
		return this.req('GET', `/api/v1/tasks/counts${query({ project_id: projectId, top_level: topLevel })}`, undefined, ACTOR);
	}

	/** The task plus its subtasks and its history. Takes a key or an id. */
	getTask(key: string): Promise<TaskDetail> {
		return this.req('GET', `/api/v1/tasks/${encodeURIComponent(key)}`, undefined, ACTOR);
	}

	/**
	 * Only the fields in `body` change. `assignee: null` clears the assignee; leaving
	 * the field out keeps it, which is what the daemon's double option means.
	 */
	updateTask(key: string, body: TaskUpdate): Promise<Task> {
		return this.req('PATCH', `/api/v1/tasks/${encodeURIComponent(key)}`, body, ACTOR);
	}

	deleteTask(key: string): Promise<void> {
		return this.req('DELETE', `/api/v1/tasks/${encodeURIComponent(key)}`, undefined, ACTOR);
	}

	/**
	 * `expectedUpdatedAt` turns a concurrent edit into a 409 instead of a clobber.
	 * `position` places the task in the column (a drag); the same stage with a position is a reorder.
	 */
	moveTask(key: string, stage: string, expectedUpdatedAt?: Timestamp, position?: number): Promise<Task> {
		const payload: MoveBody = { stage, expected_updated_at: expectedUpdatedAt, position };
		return this.req('POST', `/api/v1/tasks/${encodeURIComponent(key)}/move`, payload, ACTOR);
	}

	commentTask(key: string, body: string): Promise<TaskEvent> {
		const payload: CommentBody = { body };
		return this.req('POST', `/api/v1/tasks/${encodeURIComponent(key)}/comment`, payload, ACTOR);
	}

	/** Assigns the task to this app's actor. `force` takes it from someone else. */
	claimTask(key: string, force?: boolean): Promise<Task> {
		const payload: ClaimBody = { force };
		return this.req('POST', `/api/v1/tasks/${encodeURIComponent(key)}/claim`, payload, ACTOR);
	}

	/** Replaces the blocker list wholesale; an empty array clears it. */
	setTaskBlockers(key: string, keys: string[]): Promise<Task> {
		const payload: BlockersBody = { blocked_by: keys };
		return this.req('PUT', `/api/v1/tasks/${encodeURIComponent(key)}/blockers`, payload, ACTOR);
	}

	/** The stages in force: the project's own when it has some, the global list otherwise. */
	boardStages(projectId?: Uuid | null): Promise<StageList> {
		return this.req('GET', `/api/v1/board/stages${query({ project_id: projectId })}`, undefined, ACTOR);
	}

	/** `renames` maps an old stage name to its new one, so the tasks in it follow. */
	setBoardStages(stages: Stage[], renames?: Record<string, string>): Promise<Stage[]> {
		const payload: SetStagesBody = { stages, renames };
		return this.req('PUT', '/api/v1/board/stages', payload, ACTOR);
	}

	/** `stages: null` drops the override and puts the project back on the global list. */
	setProjectStages(projectId: Uuid, stages: Stage[] | null, renames?: Record<string, string>): Promise<StageList> {
		const payload: SetProjectStagesBody = { stages, renames };
		return this.req('PUT', `/api/v1/projects/${encodeURIComponent(projectId)}/stages`, payload, ACTOR);
	}

	/** Every framework detected in the project, each with the documents it holds. */
	listFrameworks(projectId: Uuid): Promise<FrameworkListing[]> {
		return this.req('GET', `/api/v1/projects/${encodeURIComponent(projectId)}/frameworks`);
	}

	/**
	 * Imports the framework's tasks onto the board, or its decisions as pending
	 * memories. A write, recorded under this app's actor like every board request.
	 */
	importFramework(projectId: Uuid, kind: FrameworkKind, what: ImportWhat): Promise<ImportReport> {
		const payload: FrameworkImportBody = { what };
		return this.req('POST', `/api/v1/projects/${encodeURIComponent(projectId)}/frameworks/${encodeURIComponent(kind)}/import`, payload, ACTOR);
	}

	/**
	 * Every skill that applies: the Atlas-native ones plus the `SKILL.md` folders the
	 * daemon discovers. With `projectId` the project's own roots are searched too and
	 * each row carries `enabled_here`.
	 */
	listSkills(projectId?: Uuid | null): Promise<SkillList> {
		return this.req('GET', `/api/v1/skills${query({ project_id: projectId })}`);
	}

	/** Creates an Atlas-native skill, global or scoped to one project. */
	createSkill(input: NewSkill): Promise<Skill> {
		return this.req('POST', '/api/v1/skills', input);
	}

	/** One skill with its body and the other files in its folder. */
	getSkill(id: string, projectId?: Uuid | null): Promise<Skill> {
		return this.req('GET', `/api/v1/skills/${encodeURIComponent(id)}${query({ project_id: projectId })}`);
	}

	/**
	 * Edit in place: rewrites a native skill's body, or the `SKILL.md` on disk. A
	 * project-scoped discovered skill is only reachable with `projectId`, since the
	 * daemon resolves the id against a listing of that project's roots taken right now.
	 */
	updateSkillBody(id: string, body: string, projectId?: Uuid | null): Promise<Skill> {
		const payload: SkillBodyBody = { body };
		return this.req('PUT', `/api/v1/skills/${encodeURIComponent(id)}${query({ project_id: projectId })}`, payload);
	}

	/**
	 * Name and description of a native skill. Native ids are bare UUIDs, so this route
	 * takes no `project_id`.
	 */
	patchSkill(id: string, patch: SkillPatch): Promise<Skill> {
		return this.req('PATCH', `/api/v1/skills/${encodeURIComponent(id)}`, patch);
	}

	/**
	 * Native skills only; a discovered one belongs to the folder it came from. Native ids
	 * are bare UUIDs, so this route takes no `project_id` either.
	 */
	deleteSkill(id: string): Promise<void> {
		return this.req('DELETE', `/api/v1/skills/${encodeURIComponent(id)}`);
	}

	/** Replaces the project's disabled skill list wholesale; an empty list clears it. */
	setProjectSkills(id: Uuid, disabled: string[]): Promise<Project> {
		const payload: SkillsDisabledBody = { disabled };
		return this.req('PUT', `/api/v1/projects/${encodeURIComponent(id)}/skills`, payload);
	}

	listAgents(): Promise<Persona[]> {
		return this.req('GET', '/api/v1/agents');
	}

	/** Every agent write is audited against this app's actor, like a board write. */
	createAgent(input: NewPersona): Promise<Persona> {
		return this.req('POST', '/api/v1/agents', input, ACTOR);
	}

	/** Takes an id, a slug or a name. */
	getAgent(idOrSlug: string): Promise<Persona> {
		return this.req('GET', `/api/v1/agents/${encodeURIComponent(idOrSlug)}`);
	}

	/** Only the fields in `patch` change. Takes the id, not the slug. */
	updateAgent(id: Uuid, patch: PersonaPatch): Promise<Persona> {
		return this.req('PUT', `/api/v1/agents/${encodeURIComponent(id)}`, patch, ACTOR);
	}

	deleteAgent(id: Uuid): Promise<void> {
		return this.req('DELETE', `/api/v1/agents/${encodeURIComponent(id)}`, undefined, ACTOR);
	}

	/** The agent with everything it references resolved; missing references are warnings. */
	getAgentBundle(id: string, projectId?: Uuid | null): Promise<PersonaBundle> {
		return this.req('GET', `/api/v1/agents/${encodeURIComponent(id)}/bundle${query({ project_id: projectId })}`);
	}

	getProjectRoster(projectId: Uuid): Promise<RosterRow[]> {
		return this.req('GET', `/api/v1/projects/${encodeURIComponent(projectId)}/agents`);
	}

	/** Replaces the project's roster wholesale: ids, the default and the order. */
	setProjectRoster(projectId: Uuid, entries: RosterEntry[]): Promise<RosterRow[]> {
		return this.req('PUT', `/api/v1/projects/${encodeURIComponent(projectId)}/agents`, entries, ACTOR);
	}

	/**
	 * Cross-entity search: tasks, memories, projects, files, commits, events and
	 * workflows in one call. Named `globalSearch` because `search` above is already
	 * the memories recall route.
	 */
	globalSearch(q: GlobalSearchQuery): Promise<SearchResult> {
		return this.req('GET', `/api/v1/search${query({ q: q.q, project_id: q.project_id, kinds: q.kinds, limit: q.limit })}`);
	}

	/** Transports, counts, the tools table, resources, prompts and connected clients. */
	mcpStatus(): Promise<McpStatusReport> {
		return this.req('GET', '/api/v1/mcp/status');
	}

	/** The stdio shim's registration; the desktop never calls it. */
	registerMcpClient(input: RegisterMcpClientBody): Promise<McpClient> {
		return this.req('POST', '/api/v1/mcp/clients', input);
	}

	heartbeatMcpClient(id: string, toolCalls: number): Promise<McpClient> {
		const payload: McpHeartbeatBody = { tool_calls: toolCalls };
		return this.req('PUT', `/api/v1/mcp/clients/${encodeURIComponent(id)}`, payload);
	}

	unregisterMcpClient(id: string): Promise<void> {
		return this.req('DELETE', `/api/v1/mcp/clients/${encodeURIComponent(id)}`);
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
		const payload: McpToolsBody = { disabled };
		return this.req('PUT', `/api/v1/projects/${encodeURIComponent(id)}/mcp/tools`, payload);
	}

	listPluginTools(): Promise<PluginToolDecl[]> {
		return this.req('GET', '/api/v1/mcp/plugin-tools');
	}

	/** Replaces the plugin's whole tool set; each decl's `plugin_id` is taken from the path. */
	putPluginTools(pluginId: string, tools: PluginToolDecl[]): Promise<void> {
		const payload: PluginToolsBody = { tools };
		return this.req('PUT', `/api/v1/mcp/plugin-tools/${encodeURIComponent(pluginId)}`, payload);
	}

	deletePluginTools(pluginId: string): Promise<void> {
		return this.req('DELETE', `/api/v1/mcp/plugin-tools/${encodeURIComponent(pluginId)}`);
	}

	callPluginTool(pluginId: string, name: string, args?: unknown): Promise<unknown> {
		const payload: PluginToolCallBody = { args };
		return this.req('POST', `/api/v1/mcp/plugin-tools/${encodeURIComponent(pluginId)}/${encodeURIComponent(name)}/call`, payload, ACTOR);
	}

	/**
	 * Every MCP server the user's agents are wired to. With no project that is the
	 * user-scope entries of each agent; with one it is that project's own scopes plus
	 * the plugin servers and Atlas, which apply everywhere.
	 */
	listMcpServers(projectId?: Uuid | null): Promise<McpServerList> {
		return this.req('GET', `/api/v1/mcp/servers${query({ project_id: projectId })}`);
	}

	/** Writes a new entry into the agent's own config file. */
	addMcpServer(input: NewMcpServer): Promise<McpServerEntry> {
		return this.req('POST', '/api/v1/mcp/servers', input);
	}

	/** Starts the server the way its agent would and asks it for its tools. */
	checkMcpServer(id: string, projectId?: Uuid | null): Promise<McpCheckResult> {
		return this.req('POST', `/api/v1/mcp/servers/${encodeURIComponent(id)}/check${query({ project_id: projectId })}`);
	}

	/** Flips the agent's own switch for one server; only offered where it has one. */
	setMcpServerEnabled(id: string, enabled: boolean, projectId?: Uuid | null): Promise<void> {
		const payload: McpEnabledBody = { enabled };
		return this.req('PUT', `/api/v1/mcp/servers/${encodeURIComponent(id)}/enabled${query({ project_id: projectId })}`, payload);
	}

	/** Deletes the entry from the config file it came from. */
	removeMcpServer(id: string, projectId?: Uuid | null): Promise<void> {
		return this.req('DELETE', `/api/v1/mcp/servers/${encodeURIComponent(id)}${query({ project_id: projectId })}`);
	}
}
