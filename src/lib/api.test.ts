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
		const agent = {
			name: 'writer',
			description: 'writes',
			instructions: 'write well',
			model_hint: null,
			tools: [],
			tags: []
		};
		const calls = stubFetch([{ status: 201, body: { ...agent, id: 'a', version: 1 } }]);

		const saved = await api().saveAgent(agent);

		expect(saved.name).toBe('writer');
		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/agents');
		expect(calls[0].init.method).toBe('POST');
		expect(JSON.parse(calls[0].init.body as string)).toEqual(agent);
	});

	it('deletes an agent and resolves on 204 with no body to parse', async () => {
		const calls = stubFetch([{ status: 204 }]);

		await expect(api().deleteAgent('writer')).resolves.toBeUndefined();

		expect(calls[0].url).toBe('http://127.0.0.1:7433/api/v1/agents/writer');
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
