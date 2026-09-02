// Typed client for the daemon's JSON API (crates/atlasd/src/http.rs). This is the
// only way the GUI reaches Atlas data: it never opens the DuckDB file and never
// shells out to `atlas`.

import type {
	Agent,
	Doc,
	DocKind,
	Memory,
	MemoryStatus,
	NewAgent,
	NewDoc,
	NewMemory,
	Project,
	ProjectContext,
	RecallHit,
	RecallQuery,
	Settings,
	StatusReport,
	SyncReport,
	SyncRequest,
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

	// ---- transport ----

	private async req<T>(method: string, path: string, body?: unknown): Promise<T> {
		const headers: Record<string, string> = { Accept: 'application/json' };
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
