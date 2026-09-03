// @vitest-environment jsdom
// The palette store's three guards: one request per burst of keystrokes, a stale
// response that never lands, and a recent list that stays short and unique.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SearchHit, SearchResult } from '$lib/types';

const goto = vi.fn();
vi.mock('$app/navigation', () => ({ goto: (href: string) => goto(href) }));

const mocks = vi.hoisted(() => ({ globalSearch: vi.fn(), listProjects: vi.fn() }));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { filterCommands } from './commands';
import {
	closePalette,
	hrefFor,
	openItem,
	palette,
	pushRecent,
	RECENT_KEY,
	RECENT_MAX,
	runSearch,
	setInput
} from './palette.svelte';

function hit(id: string, title: string, over: Partial<SearchHit> = {}): SearchHit {
	return {
		kind: 'task',
		id,
		title,
		subtitle: null,
		project_id: null,
		reference: null,
		score: 3,
		highlights: [],
		...over
	};
}

function result(title: string): SearchResult {
	return { groups: [{ kind: 'task', items: [hit('t1', title)] }], total: 1, took_ms: 4 };
}

beforeEach(() => {
	vi.useRealTimers();
	goto.mockReset();
	mocks.globalSearch.mockReset();
	mocks.listProjects.mockReset();
	localStorage.clear();
	closePalette();
	palette.recent = [];
});

describe('debounced search', () => {
	it('coalesces a burst of keystrokes into one request', async () => {
		vi.useFakeTimers();
		mocks.globalSearch.mockResolvedValue(result('one'));

		setInput('duck');
		setInput('duckdb');
		expect(mocks.globalSearch).not.toHaveBeenCalled();

		await vi.advanceTimersByTimeAsync(120);

		expect(mocks.globalSearch).toHaveBeenCalledTimes(1);
		expect(mocks.globalSearch).toHaveBeenCalledWith({ q: 'duckdb', kinds: undefined });
	});

	it('asks for one kind when a prefix narrows the query', async () => {
		vi.useFakeTimers();
		mocks.globalSearch.mockResolvedValue(result('one'));

		setInput('#ready');
		await vi.advanceTimersByTimeAsync(120);

		expect(mocks.globalSearch).toHaveBeenCalledWith({ q: 'ready', kinds: ['task'] });
	});

	it('sends nothing for a command query', async () => {
		vi.useFakeTimers();
		setInput('>tog');
		await vi.advanceTimersByTimeAsync(120);
		expect(mocks.globalSearch).not.toHaveBeenCalled();
	});
});

describe('the generation guard', () => {
	it('drops a response that lands after a newer one went out', async () => {
		let landFirst: (r: SearchResult) => void = () => {};
		mocks.globalSearch
			.mockImplementationOnce(
				() =>
					new Promise<SearchResult>((resolve) => {
						landFirst = resolve;
					})
			)
			.mockImplementationOnce(async () => result('second'));

		palette.input = 'a';
		const first = runSearch();
		palette.input = 'b';
		await runSearch();

		landFirst(result('first'));
		await first;

		expect(palette.results?.groups[0].items[0].title).toBe('second');
	});
});

describe('recent', () => {
	it('caps at eight, newest first', () => {
		for (let i = 0; i < 10; i++) pushRecent(hit(`t${i}`, `title ${i}`));

		expect(palette.recent).toHaveLength(RECENT_MAX);
		expect(palette.recent[0].id).toBe('t9');
		expect(JSON.parse(localStorage.getItem(RECENT_KEY) ?? '[]')).toHaveLength(RECENT_MAX);
	});

	it('dedupes by kind and id, moving the repeat to the front', () => {
		pushRecent(hit('t1', 'one'));
		pushRecent(hit('t2', 'two'));
		pushRecent(hit('t1', 'one'));

		expect(palette.recent.map((h) => h.id)).toEqual(['t1', 't2']);
	});
});

describe('commands', () => {
	it('filters by case-insensitive substring over the label and the hint', () => {
		expect(filterCommands('THEME').map((c) => c.id)).toEqual(['toggle-theme']);
		expect(filterCommands('by hand').map((c) => c.id)).toEqual(['remember']);
		expect(filterCommands('')).toHaveLength(filterCommands('').length);
	});
});

describe('opening an item', () => {
	it('sends a task to its project board with the key selected', async () => {
		await openItem(hit('id-1', 'Move DuckDB calls', { project_id: 'p1', reference: 'ATL-2' }));
		expect(goto).toHaveBeenCalledWith('/projects/p1/board?task=ATL-2');
	});

	it('sends a global task to the projects list', () => {
		expect(hrefFor(hit('id-1', 'x'))).toBe('/projects');
	});

	it('opens a memory filtered by id, and adds the inspector flag', () => {
		const memory = hit('m1', 'a memory', { kind: 'memory' });
		expect(hrefFor(memory)).toBe('/memories?id=m1');
		expect(hrefFor(memory, 'inspector')).toBe('/memories?id=m1&inspector=1');
	});

	it('opens the board filtered by the query text', () => {
		palette.input = 'duckdb';
		expect(hrefFor(hit('id-1', 'x', { project_id: 'p1' }), 'board')).toBe(
			'/projects/p1/board?q=duckdb'
		);
	});
});
