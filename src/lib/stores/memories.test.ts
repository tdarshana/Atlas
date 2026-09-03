// The kind filter is a client-side view over one load. These tests pin that: the fetch
// never carries `kinds`, `all` keeps every row the load returned, and toggling a kind
// re-derives `hits` without going back to the daemon. The side panel counts `all`, so a
// second request here would double the largest payload in the app.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Memory, MemoryKind } from '$lib/types';

const mocks = vi.hoisted(() => ({ listMemories: vi.fn(), search: vi.fn(), forget: vi.fn() }));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { clearKinds, loadMemories, memories, toggleKind } from './memories.svelte';

function memory(id: string, kind: MemoryKind): Memory {
	return {
		id,
		scope: 'global',
		project_id: null,
		kind,
		text: `memory ${id}`,
		tags: [],
		source_agent: null,
		source_tool: null,
		confidence: 1,
		status: 'active',
		superseded_by: null,
		created_at: '2026-09-01T00:00:00Z',
		updated_at: '2026-09-01T00:00:00Z'
	};
}

const rows = [memory('a', 'fact'), memory('b', 'decision'), memory('c', 'fact')];

beforeEach(() => {
	mocks.listMemories.mockReset();
	mocks.search.mockReset();
	mocks.listMemories.mockResolvedValue(rows);
	memories.query = '';
	memories.scope = 'all';
	memories.projectId = '';
	memories.kinds = [];
	memories.all = [];
	memories.hits = [];
	memories.selected = null;
});

describe('memories store', () => {
	it('keeps every row in `all` and narrows only `hits` by kind', async () => {
		await loadMemories();
		expect(memories.all).toHaveLength(3);
		expect(memories.hits).toHaveLength(3);

		toggleKind('fact');
		expect(memories.hits.map((h) => h.memory.id)).toEqual(['a', 'c']);
		// The panel's facets read `all`, so `decision` still has a row to widen back to.
		expect(memories.all).toHaveLength(3);
	});

	it('costs no extra request when a kind is toggled or cleared', async () => {
		await loadMemories();
		expect(mocks.listMemories).toHaveBeenCalledTimes(1);

		toggleKind('fact');
		toggleKind('decision');
		clearKinds();

		expect(mocks.listMemories).toHaveBeenCalledTimes(1);
		expect(memories.hits).toHaveLength(3);
	});

	it('never sends the kind filter to the search route', async () => {
		mocks.search.mockResolvedValue(rows.map((m) => ({ memory: m, score: 1 })));
		memories.query = 'duck';
		memories.kinds = ['fact'];

		await loadMemories();

		const sent = mocks.search.mock.calls[0][0] as Record<string, unknown>;
		expect(sent.kinds).toBeUndefined();
		expect(memories.all).toHaveLength(3);
		expect(memories.hits.map((h) => h.memory.id)).toEqual(['a', 'c']);
	});
});
