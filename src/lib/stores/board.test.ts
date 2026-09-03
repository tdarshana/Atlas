// The board store's rules: how columns fall out of stages plus tasks, what makes a
// stage list usable, and the two guards, one on loading and one on moving a card.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '$lib/api';
import type { Stage, Task } from '$lib/types';

const mocks = vi.hoisted(() => ({
	listTasks: vi.fn(),
	boardStages: vi.fn(),
	moveTask: vi.fn(),
	getTask: vi.fn()
}));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { board, deriveColumns, move, refresh, validateStages } from './board.svelte';

const STAGES: Stage[] = [
	{ name: 'Backlog', done: false },
	{ name: 'In Progress', done: false },
	{ name: 'Done', done: true }
];

function task(key: string, stage: string): Task {
	return {
		id: `id-${key}`,
		key,
		project_id: null,
		seq: 1,
		title: key,
		description: '',
		stage,
		kind: 'task',
		priority: 'medium',
		assignee: null,
		labels: [],
		parent_id: null,
		created_by: 'desktop',
		created_at: '2026-09-03T10:00:00Z',
		updated_at: '2026-09-03T10:00:00Z',
		closed_at: null,
		blocked_by: [],
		ready: true,
		blocked_reason: null
	};
}

beforeEach(() => {
	vi.clearAllMocks();
	board.stages = STAGES.map((s) => ({ ...s }));
	board.tasks = [];
	board.filters.projectId = null;
	board.filters.assignee = '';
	board.filters.query = '';
	board.filters.showDone = false;
	board.error = null;
	board.selected = null;
});

describe('deriveColumns', () => {
	it('puts every task under its stage and keeps the stage order', () => {
		const tasks = [task('ATL-1', 'Done'), task('ATL-2', 'Backlog'), task('ATL-3', 'Done')];

		const columns = deriveColumns(STAGES, tasks);

		expect(columns.map((c) => c.stage.name)).toEqual(['Backlog', 'In Progress', 'Done']);
		expect(columns[0].tasks.map((t) => t.key)).toEqual(['ATL-2']);
		expect(columns[1].tasks).toEqual([]);
		expect(columns[2].tasks.map((t) => t.key)).toEqual(['ATL-1', 'ATL-3']);
		expect(columns.some((c) => c.strays)).toBe(false);
	});

	it('parks a task whose stage is gone in the first column and says so', () => {
		const columns = deriveColumns(STAGES, [task('ATL-9', 'Retired')]);

		expect(columns[0].tasks.map((t) => t.key)).toEqual(['ATL-9']);
		expect(columns[0].strays).toBe(true);
		expect(columns[1].strays).toBe(false);
	});

	it('has no columns when there are no stages, and drops nothing on the floor', () => {
		expect(deriveColumns([], [task('ATL-1', 'Backlog')])).toEqual([]);
	});
});

describe('validateStages', () => {
	it('accepts a workable list', () => {
		expect(validateStages(STAGES)).toBeNull();
	});

	it('rejects a board with one stage', () => {
		expect(validateStages([{ name: 'Done', done: true }])).toMatch(/between 2 and 12/);
	});

	it('rejects two stages with the same name', () => {
		const stages: Stage[] = [
			{ name: 'Backlog', done: false },
			{ name: 'backlog', done: false },
			{ name: 'Done', done: true }
		];
		expect(validateStages(stages)).toMatch(/Two stages/);
	});

	it('rejects a list with no done stage', () => {
		const stages: Stage[] = [
			{ name: 'Backlog', done: false },
			{ name: 'In Progress', done: false }
		];
		expect(validateStages(stages)).toMatch(/done stage/);
	});

	it('rejects a blank name', () => {
		const stages: Stage[] = [
			{ name: '  ', done: false },
			{ name: 'Done', done: true }
		];
		expect(validateStages(stages)).toMatch(/needs a name/);
	});
});

describe('refresh', () => {
	it('drops a load that finishes after a newer one', async () => {
		let release: (tasks: Task[]) => void = () => {};
		const slow = new Promise<Task[]>((resolve) => (release = resolve));
		mocks.boardStages.mockResolvedValue({ stages: STAGES, overridden: false });
		mocks.listTasks.mockReturnValueOnce(slow).mockResolvedValueOnce([task('ATL-2', 'Backlog')]);

		const first = refresh();
		const second = refresh();
		await second;
		release([task('ATL-1', 'Backlog')]);
		await first;

		expect(board.tasks.map((t) => t.key)).toEqual(['ATL-2']);
	});
});

describe('move', () => {
	it('moves the card before the daemon answers and keeps the row it sends back', async () => {
		board.tasks = [task('ATL-1', 'Backlog')];
		const moved = { ...task('ATL-1', 'Done'), updated_at: '2026-09-03T11:00:00Z' };
		mocks.moveTask.mockResolvedValue(moved);

		const pending = move('ATL-1', 'Done');
		expect(board.tasks[0].stage).toBe('Done');
		await pending;

		expect(mocks.moveTask).toHaveBeenCalledWith('ATL-1', 'Done', '2026-09-03T10:00:00Z');
		expect(board.tasks[0].updated_at).toBe('2026-09-03T11:00:00Z');
	});

	it('puts the card back when the daemon refuses the move', async () => {
		board.tasks = [task('ATL-1', 'Backlog')];
		mocks.moveTask.mockRejectedValue(new ApiError('stage "Done" is not on this board', 400));

		const pending = move('ATL-1', 'Done');
		expect(board.tasks[0].stage).toBe('Done');
		await pending;

		expect(board.tasks[0].stage).toBe('Backlog');
	});

	it('reloads when someone else changed the task first', async () => {
		board.tasks = [task('ATL-1', 'Backlog')];
		mocks.moveTask.mockRejectedValue(new ApiError('task changed', 409));
		mocks.boardStages.mockResolvedValue({ stages: STAGES, overridden: false });
		mocks.listTasks.mockResolvedValue([task('ATL-1', 'In Progress')]);

		await move('ATL-1', 'Done');

		expect(mocks.listTasks).toHaveBeenCalledTimes(1);
		expect(board.tasks[0].stage).toBe('In Progress');
	});
});
