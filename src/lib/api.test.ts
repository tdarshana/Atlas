import { afterEach, describe, expect, it } from 'vitest';
import { ApiError, AtlasApi } from './api';

type Call = { url: string; init: RequestInit };

/** Records every fetch and answers each with the next queued response. */
function stubFetch(responses: Array<{ status: number; body?: unknown }>): Call[] {
	const calls: Call[] = [];
	let i = 0;
	globalThis.fetch = (async (url: string | URL | Request, init: RequestInit = {}) => {
		calls.push({ url: String(url), init });
		const r = responses[i++] ?? { status: 200, body: {} };
		const text = r.body === undefined ? '' : JSON.stringify(r.body);
		return new Response(r.status === 204 ? null : text, {
			status: r.status,
			headers: { 'content-type': 'application/json' }
		});
	}) as typeof fetch;
	return calls;
}

const realFetch = globalThis.fetch;
afterEach(() => {
	globalThis.fetch = realFetch;
});

const api = () => new AtlasApi('http://127.0.0.1:7433');

describe('AtlasApi', () => {
	// SEC-5: the daemon refuses any `/api/v1` request without its token, so the client
	// sends it on every kind of request it makes, and none when it has none to send.
	it('sends the daemon token as X-Atlas-Token on every request', async () => {
		const calls = stubFetch([{ status: 200, body: {} }, { status: 200, body: [] }, { status: 200, body: '' }]);
		const withToken = new AtlasApi('http://127.0.0.1:7433', 'secret-token');

		await withToken.status();
		await withToken.search({ query: 'x', limit: 1, kinds: [], tags: [] });
		await withToken.projectLogExport('p1');

		expect(calls).toHaveLength(3);
		for (const call of calls) {
			expect(new Headers(call.init.headers).get('x-atlas-token')).toBe('secret-token');
		}

		const bare = stubFetch([{ status: 200, body: {} }]);
		await api().status();
		expect(new Headers(bare[0].init.headers).get('x-atlas-token')).toBeNull();
	});

	it('asks for a new token once on a 401 and retries with it', async () => {
		const calls = stubFetch([{ status: 401, body: { error: 'unauthorized' } }, { status: 200, body: { ok: true } }]);
		let asked = 0;
		const client = new AtlasApi('http://127.0.0.1:7433', 'stale', async () => {
			asked++;
			return 'fresh';
		});
		await client.status();
		expect(asked).toBe(1);
		expect(calls).toHaveLength(2);
		expect(new Headers(calls[0].init.headers).get('x-atlas-token')).toBe('stale');
		expect(new Headers(calls[1].init.headers).get('x-atlas-token')).toBe('fresh');

		// A second 401 after the refresh is an error, not a loop; no refresh means no retry.
		stubFetch([{ status: 401, body: { error: 'unauthorized' } }, { status: 401, body: { error: 'unauthorized' } }]);
		await expect(client.status()).rejects.toMatchObject({ status: 401 });
		const noReauth = stubFetch([{ status: 401, body: { error: 'unauthorized' } }]);
		await expect(new AtlasApi('http://127.0.0.1:7433', 'stale').status()).rejects.toMatchObject({ status: 401 });
		expect(noReauth).toHaveLength(1);
	});

	it('posts a search to /api/v1/memories/search with the query as the JSON body', async () => {
		const calls = stubFetch([{ status: 200, body: [] }]);
		const q = { query: 'hello', limit: 5, kinds: ['fact' as const], tags: [] };

		const hits = await api().search(q);

		expect(hits).toEqual([]);
		expect(calls).toHaveLength(1);
		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/memories/search');
		expect(calls[0].init.method).toBe('POST');
		expect(JSON.parse(calls[0].init.body as string)).toEqual(q);
		const headers = new Headers(calls[0].init.headers);
		expect(headers.get('accept')).toBe('application/json');
		expect(headers.get('content-type')).toBe('application/json');
	});

	it('posts an agent to /api/v1/agents', async () => {
		const agent = { name: 'Writer', role: 'writes', instructions: 'write well', tags: [] };
		const calls = stubFetch([{ status: 201, body: { ...agent, id: 'a', slug: 'writer' } }]);

		const saved = await api().createAgent(agent);

		expect(saved.slug).toBe('writer');
		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/agents');
		expect(calls[0].init.method).toBe('POST');
		expect(JSON.parse(calls[0].init.body as string)).toEqual(agent);
	});

	it('deletes an agent and resolves on 204 with no body to parse', async () => {
		const calls = stubFetch([{ status: 204 }]);

		await expect(api().deleteAgent('a')).resolves.toBeUndefined();

		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/agents/a');
		expect(calls[0].init.method).toBe('DELETE');
	});

	it('throws ApiError carrying the error field and the status on 400', async () => {
		stubFetch([{ status: 400, body: { error: 'bad' } }]);

		const err = await api()
			.listProjects()
			.catch((e) => e);

		expect(err).toBeInstanceOf(ApiError);
		expect(err.message).toBe('bad');
		expect(err.status).toBe(400);
	});

	it('encodes the status and project filters on GET /api/v1/memories', async () => {
		const calls = stubFetch([{ status: 200, body: [] }]);

		await api().listMemories('pending', 'p1');

		expect(calls[0].url).toBe(
			'http://127.0.0.1:7433/api/v1/memories?status=pending&project_id=p1'
		);
		expect(calls[0].init.method ?? 'GET').toBe('GET');
	});

	it('encodes limit and offset on GET /api/v1/memories, offset 0 included', async () => {
		const calls = stubFetch([{ status: 200, body: [] }, { status: 200, body: [] }]);

		await api().listMemories('active', null, undefined, { limit: 200, offset: 0 });
		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/memories?status=active&limit=200&offset=0');

		await api().listMemories('active', undefined, undefined, { limit: 10 });
		expect(calls[1].url).toBe('http://127.0.0.1:7433/api/v1/memories?status=active&limit=10');
	});

	it('resolves testExtraction with the ok:false body on a 400 connectivity failure', async () => {
		stubFetch([{ status: 400, body: { ok: false, error: 'model call failed' } }]);

		const result = await api().testExtraction();

		expect(result).toEqual({ ok: false, error: 'model call failed' });
	});

	it('throws ApiError on a 409 when extraction is disabled', async () => {
		stubFetch([{ status: 409, body: { error: 'extraction is disabled' } }]);

		const err = await api()
			.testExtraction()
			.catch((e) => e);

		expect(err).toBeInstanceOf(ApiError);
		expect(err.message).toBe('extraction is disabled');
		expect(err.status).toBe(409);
	});

	it('sends the ingest actor as X-Atlas-Actor, not as a source_tool body field', async () => {
		const calls = stubFetch([{ status: 202, body: { job_id: 'j1' } }]);

		await api().ingest('some transcript text', 'claude-code', 'p1');

		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/ingest');
		expect(calls[0].init.method).toBe('POST');
		const headers = new Headers(calls[0].init.headers);
		expect(headers.get('X-Atlas-Actor')).toBe('claude-code');
		expect(JSON.parse(calls[0].init.body as string)).toEqual({
			text: 'some transcript text',
			project_root: 'p1'
		});
	});

	it('maps doc kinds onto the plural routes', async () => {
		const calls = stubFetch([
			{ status: 200, body: [] },
			{ status: 200, body: [] }
		]);

		await api().listDocs('practice');
		await api().listDocs('workflow', 'p1');

		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/practices');
		expect(calls[1].url).toBe('http://127.0.0.1:7433/api/v1/workflows?project_id=p1');
	});
});

describe('board requests', () => {
	/** The header the daemon records as the actor on every event the app causes. */
	const actor = (call: Call) => new Headers(call.init.headers).get('X-Atlas-Actor');

	it('sends X-Atlas-Actor: desktop on a board write', async () => {
		const calls = stubFetch([{ status: 200, body: {} }]);

		await api().moveTask('ATL-1', 'Done', '2026-09-03T10:00:00Z');

		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/tasks/ATL-1/move');
		expect(calls[0].init.method).toBe('POST');
		expect(actor(calls[0])).toBe('desktop');
		expect(JSON.parse(calls[0].init.body as string)).toEqual({
			stage: 'Done',
			expected_updated_at: '2026-09-03T10:00:00Z'
		});
	});

	it('sends the actor on the board reads too', async () => {
		const calls = stubFetch([
			{ status: 200, body: [] },
			{ status: 200, body: { stages: [], overridden: false } },
			{ status: 200, body: [] }
		]);

		await api().listTasks({ assignee: 'alice', include_done: true });
		await api().boardStages();
		await api().taskCounts();

		expect(calls.map((c) => actor(c))).toEqual(['desktop', 'desktop', 'desktop']);
		expect(calls[0].url).toBe(
			'http://127.0.0.1:7433/api/v1/tasks?assignee=alice&include_done=true'
		);
		expect(calls[2].url).toBe('http://127.0.0.1:7433/api/v1/tasks/counts');
	});

	it('omits an absent assignee and clears one that is null', async () => {
		const calls = stubFetch([
			{ status: 200, body: {} },
			{ status: 200, body: {} }
		]);

		await api().updateTask('ATL-1', { title: 'renamed' });
		await api().updateTask('ATL-1', { assignee: null });

		expect(calls[0].init.method).toBe('PATCH');
		expect(JSON.parse(calls[0].init.body as string)).toEqual({ title: 'renamed' });
		expect(JSON.parse(calls[1].init.body as string)).toEqual({ assignee: null });
	});
});
