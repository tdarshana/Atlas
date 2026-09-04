import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
	ACTIVE_WINDOW_DAYS,
	activeAgents,
	hubTitle,
	idForPath,
	openProject,
	project,
	tabForPath,
	tabsFor,
	taskSummary,
	treeRows
} from './project.svelte';
import { projectDetail } from './projects.svelte';
import type { Project, Task } from '$lib/types';

// The hub load goes through the projects store, which reaches the daemon.
const daemon = vi.hoisted(() => ({ calls: 0, fail: true }));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({
		getProject: async (id: string) => {
			daemon.calls++;
			if (daemon.fail) throw new Error('no such project');
			return { id, name: 'atlas', root_path: '/tmp/atlas' } as Project;
		},
		projectContext: async () => ({ memories: [], practices: [], workflows: [] })
	})
}));

const DAY_MS = 86_400_000;

function task(over: Partial<Task>): Task {
	return {
		id: 'id',
		key: 'ATL-1',
		project_id: null,
		seq: 1,
		title: 'A task',
		description: '',
		stage: 'Backlog',
		kind: 'task',
		priority: 'medium',
		assignee: null,
		labels: [],
		parent_id: null,
		created_by: 'desktop',
		created_at: '2026-09-01T00:00:00Z',
		updated_at: '2026-09-01T00:00:00Z',
		closed_at: null,
		blocked_by: [],
		open_blockers: 0,
		ready: true,
		blocked_reason: null,
		subtasks_total: 0,
		subtasks_done: 0,
		...over
	};
}

describe('project tabs', () => {
	it('reads the open tab from the route', () => {
		expect(tabForPath('/projects/x/log')).toBe('log');
		expect(tabForPath('/projects/x')).toBe('profile');
		expect(tabForPath('/projects/x/board')).toBe('board');
		expect(tabForPath('/projects/x/skills')).toBe('skills');
		expect(tabForPath('/projects/x/settings')).toBe('settings');
	});

	it('falls back to Profile for a segment the hub does not have', () => {
		expect(tabForPath('/projects/x/nowhere')).toBe('profile');
	});

	it('reads the project id from the route', () => {
		expect(idForPath('/projects/x/board')).toBe('x');
		expect(idForPath('/memories')).toBe('');
	});

	it('gives a project all twelve tabs, in the frame order', () => {
		const tabs = tabsFor('x');
		expect(tabs.map((t) => t.id)).toEqual([
			'profile',
			'board',
			'memories',
			'practices',
			'skills',
			'agents',
			'workflows',
			'frameworks',
			'mcp',
			'permissions',
			'log',
			'settings'
		]);
		expect(tabs[0].href).toBe('/projects/x');
		expect(tabs[1].href).toBe('/projects/x/board');
	});

	it('gives global only Board and Memories', () => {
		const tabs = tabsFor('global');
		expect(tabs.map((t) => t.id)).toEqual(['board', 'memories']);
		expect(tabs[0].href).toBe('/projects/global/board');
	});

	it('names the project and the tab in the command box', () => {
		const project = { name: 'atlas' } as Project;
		expect(hubTitle('/projects/x/log', project)).toBe('atlas · Log');
		expect(hubTitle('/projects/global/board', null)).toBe('Global · Board');
		expect(hubTitle('/memories', project)).toBeNull();
	});
});

describe('treeRows', () => {
	it('turns paths into rows with a depth and a kind', () => {
		const rows = treeRows(['src/', 'src/main.rs', 'README.md']);
		expect(rows).toEqual([
			{ path: 'src', name: 'src', depth: 0, kind: 'folder' },
			{ path: 'src/main.rs', name: 'main.rs', depth: 1, kind: 'file' },
			{ path: 'README.md', name: 'README.md', depth: 0, kind: 'file' }
		]);
	});

	it('puts folders before files within a level', () => {
		const rows = treeRows(['a.txt', 'zeta/', 'b.txt', 'alpha/']);
		expect(rows.map((r) => r.name)).toEqual(['alpha', 'zeta', 'a.txt', 'b.txt']);
	});

	it('implies the folders a deep path hangs off', () => {
		const rows = treeRows(['crates/atlas-core/src/db.rs']);
		expect(rows.map((r) => [r.name, r.depth, r.kind])).toEqual([
			['crates', 0, 'folder'],
			['atlas-core', 1, 'folder'],
			['src', 2, 'folder'],
			['db.rs', 3, 'file']
		]);
	});
});

describe('activeAgents', () => {
	const now = Date.parse('2026-09-03T00:00:00Z');

	it('keeps the distinct actors inside the window', () => {
		const recent = new Date(now - 2 * DAY_MS).toISOString();
		const agents = activeAgents(
			[
				task({ updated_at: recent, created_by: 'cli/codex', assignee: 'cli/codex' }),
				task({ updated_at: recent, created_by: 'desktop', assignee: null })
			],
			now
		);
		expect(agents).toEqual(['cli/codex', 'desktop']);
	});

	it('drops an actor whose last touch is older than the window', () => {
		const stale = new Date(now - (ACTIVE_WINDOW_DAYS + 1) * DAY_MS).toISOString();
		expect(activeAgents([task({ updated_at: stale, created_by: 'cli/claude' })], now)).toEqual([]);
	});
});

describe('openProject', () => {
	beforeEach(() => {
		daemon.calls = 0;
		daemon.fail = true;
		projectDetail.project = null;
		projectDetail.error = null;
	});

	it('records the failure so the layout can show it instead of the tabs', async () => {
		await openProject('does-not-exist');
		expect(project.error).toBe('no such project');
		expect(project.current).toBeNull();
	});

	it('fetches again when Retry forces it, and clears the error', async () => {
		await openProject('x');
		expect(daemon.calls).toBe(1);

		daemon.fail = false;
		await openProject('x', true);
		expect(daemon.calls).toBe(2);
		expect(project.error).toBeNull();
		expect(project.current?.name).toBe('atlas');
	});

	it('does not refetch a project it already holds', async () => {
		daemon.fail = false;
		await openProject('x');
		await openProject('x');
		expect(daemon.calls).toBe(1);
	});
});

describe('taskSummary', () => {
	it('counts the tasks and those in testing', () => {
		const tasks = [task({ stage: 'Backlog' }), task({ stage: 'Testing' })];
		expect(taskSummary(tasks)).toBe('2 · 1 in testing');
	});
});
