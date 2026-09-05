// Typed client for the daemon's JSON API (crates/atlasd/src/http.rs). This is the
// only way the GUI reaches Atlas data: it never opens the DuckDB file and never
// shells out to `atlas`.
//
// The route methods are generated into ./api.generated.ts from the daemon's route
// table (crates/atlasd/src/routes.rs, `bun run gen:api`); `cargo test -p atlasd` fails
// when that file is stale. What stays here is the transport (`req` and `text`), the
// error type, and the few routes the table cannot say: the practice routes pick their
// collection from a `DocKind`, `ingest` sends a per-call actor, `testExtraction` reads
// a 400 as data, and `getFrameworkDoc` encodes its path a segment at a time.

import type {
	Doc,
	DocKind,
	ExtractionTestResult,
	FrameworkKind,
	NewDoc,
	TriggerKind,
	Uuid,
	WorkflowRun
} from './types';
import { GeneratedApi, query } from './api.generated';

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

/** Plural route segment for a doc kind: practice -> practices. */
const docPath = (kind: DocKind) => (kind === 'practice' ? 'practices' : 'workflows');

/** Asks the host for the daemon's current token after a 401; `null` when it cannot. */
export type Reauth = () => Promise<string | null>;

export class AtlasApi extends GeneratedApi {
	/** `token` is the daemon secret (SEC-5); every request goes out with it in
	 * `X-Atlas-Token`. Empty means none is known, and the daemon will answer 401.
	 * `reauth` runs once on a 401 (the daemon restarted and minted a new token) and the
	 * request is retried with what it returns. */
	constructor(
		public baseUrl: string,
		private token: string = '',
		private reauth?: Reauth
	) {
		super();
	}

	/** True when a 401 was answered with a fresh token, so the caller should retry once. */
	private async refreshed(res: Response, retried: boolean): Promise<boolean> {
		if (res.status !== 401 || retried || !this.reauth) return false;
		const next = await this.reauth();
		if (!next) return false;
		this.token = next;
		return true;
	}

	/** The headers every request starts from: `Accept`, plus the token when there is one. */
	private headers(extra?: Record<string, string>): Record<string, string> {
		const h: Record<string, string> = { Accept: 'application/json', ...extra };
		if (this.token) h['X-Atlas-Token'] = this.token;
		return h;
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

	// ---- workflows ----

	/**
	 * 202: the run is queued, not finished. Hand-written because the store still hands
	 * `input` over as `unknown`, where the daemon reads a string; a write, so it carries
	 * the app's actor like the generated workflow routes.
	 */
	runWorkflow(id: Uuid, trigger?: TriggerKind, input?: unknown): Promise<WorkflowRun> {
		return this.req(
			'POST',
			`/api/v1/workflows/${encodeURIComponent(id)}/run`,
			{ trigger, input },
			{ 'X-Atlas-Actor': 'desktop' }
		);
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
				headers: this.headers()
			});
		} catch (e) {
			throw new ApiError(e instanceof Error ? e.message : String(e), 0);
		}
		const text = await res.text();
		if (res.status === 400) return JSON.parse(text) as ExtractionTestResult;
		if (!res.ok) throw new ApiError(errorMessage(text, res), res.status);
		return JSON.parse(text) as ExtractionTestResult;
	}

	// ---- frameworks ----

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

	// ---- transport ----

	/**
	 * A route whose body is not JSON. The log export is JSONL, which `JSON.parse` would
	 * choke on, so it comes back as the text it is.
	 */
	protected async text(method: string, path: string, retried = false): Promise<string> {
		let res: Response;
		try {
			res = await fetch(`${this.baseUrl}${path}`, { method, headers: this.headers() });
		} catch (e) {
			throw new ApiError(e instanceof Error ? e.message : String(e), 0);
		}
		if (await this.refreshed(res, retried)) return this.text(method, path, true);
		const body = await res.text();
		if (!res.ok) throw new ApiError(errorMessage(body, res), res.status);
		return body;
	}

	protected async req<T>(
		method: string,
		path: string,
		body?: unknown,
		extra?: Record<string, string>,
		retried = false
	): Promise<T> {
		const headers = this.headers(extra);
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
		if (await this.refreshed(res, retried)) return this.req<T>(method, path, body, extra, true);

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
