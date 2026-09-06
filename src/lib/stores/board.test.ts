// The board store's rules: how columns fall out of stages plus tasks, what makes a
// stage list usable, and the two guards, one on loading and one on moving a card.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ApiError } from '$lib/api';
import type { Stage, Task } from '$lib/types';

const mocks = vi.hoisted(() => ({
	listTasks: vi.fn(),
	boardStages: vi.fn(),
	moveTask: vi.fn(),
	getTask: vi.fn(),
	updateTask: vi.fn(),
	setTaskBlockers: vi.fn(),
	claimTask: vi.fn(),
	commentTask: vi.fn(),
	deleteTask: vi.fn(),
	importFramework: vi.fn()
}));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import {
	backTask,
	board,
	claim,
	closeTask,
	comment,
	ConflictError,
	deriveColumns,
	importFramework,
	loadDetail,
	move,
	openTask,
	refresh,
	removeTask,
	setBlockers,
	stageRenames,
	startBoardPolling,
	updateTask,
	validateStages
} from './board.svelte';
import { dispatch } from './changes.svelte';

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
		position: 1,
		title: key,
		description: '',
		stage,
		kind: 'task',
		priority: 'medium',
		assignee: null,
		labels: [],
		parent_id: null,
		parent_key: null,
		parent_title: null,
		created_by: 'desktop',
		created_at: '2026-09-03T10:00:00Z',
		updated_at: '2026-09-03T10:00:00Z',
		closed_at: null,
		source_ref: null,
		blocked_by: [],
		open_blockers: 0,
		ready: true,
		blocked_reason: null,
		subtasks_total: 0,
		subtasks_done: 0
	};
}

beforeEach(() => {
	vi.clearAllMocks();
	board.stages = STAGES.map((s) => ({ ...s }));
	board.tasks = [];
	board.filters.projectId = null;
	board.filters.assignee = '';
	board.filters.query = '';
	board.filters.hideDone = false;
	board.error = null;
	board.selected = null;
	board.detail = null;
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

describe('stageRenames', () => {
	it('maps the name the server sent to the name in the box', () => {
		const rows = [
			{ name: 'Todo', original: 'Backlog' },
			{ name: 'In Progress', original: 'In Progress' },
			{ name: 'Shipped', original: null }
		];

		expect(stageRenames(rows)).toEqual({ Backlog: 'Todo' });
	});

	it('follows two renames in one sitting back to the original name', () => {
		// The row arrived as A, was typed to B, then to C. The daemon has to be told
		// A became C, because A is the name the tasks are standing in.
		expect(stageRenames([{ name: 'C', original: 'A' }])).toEqual({ A: 'C' });
	});

	it('ignores whitespace and rows that did not change', () => {
		const rows = [
			{ name: '  Backlog  ', original: 'Backlog' },
			{ name: 'Done', original: 'Done' }
		];

		expect(stageRenames(rows)).toEqual({});
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

	it('lists subtasks too, narrows to top-level only when asked, and never while searching', async () => {
		mocks.boardStages.mockResolvedValue({ stages: STAGES, overridden: false });
		mocks.listTasks.mockResolvedValue([]);

		await refresh();
		expect(mocks.listTasks.mock.calls.at(-1)![0].top_level).toBeUndefined();

		board.filters.hideSubtasks = true;
		await refresh();
		expect(mocks.listTasks).toHaveBeenLastCalledWith(expect.objectContaining({ top_level: true }));

		board.filters.query = '  widget  ';
		await refresh();
		const call = mocks.listTasks.mock.calls.at(-1)![0];
		expect(call.top_level).toBeUndefined();
		expect(call.query).toBe('widget');
		board.filters.hideSubtasks = false;
	});
});

describe('the board follows the daemon', () => {
	it('folds a loaded detail into the list so the card and its subtasks take the stage the panel shows', async () => {
		board.tasks = [task('ATL-1', 'Backlog'), task('ATL-2', 'Backlog'), task('ATL-3', 'Backlog')];
		mocks.getTask.mockResolvedValue({
			task: task('ATL-1', 'In Progress'),
			children: [task('ATL-2', 'Done'), task('ATL-9', 'Done')],
			events: []
		});

		await loadDetail('ATL-1');

		expect(board.tasks.map((t) => [t.key, t.stage])).toEqual([
			['ATL-1', 'In Progress'],
			['ATL-2', 'Done'],
			['ATL-3', 'Backlog']
		]);
	});

	it('asks for the brief list: the board never shows a description (PERF-5, ATL-307)', async () => {
		mocks.boardStages.mockResolvedValue({ stages: STAGES, overridden: false });
		mocks.listTasks.mockResolvedValue([]);
		await refresh();
		expect(mocks.listTasks).toHaveBeenLastCalledWith(expect.objectContaining({ brief: true }));
	});

	it('patches the one task a lone change names, re-lists a burst, and reloads the open detail when it is touched', async () => {
		vi.useFakeTimers();
		try {
			const listed = () => [task('ATL-1', 'Backlog'), task('ATL-2', 'Backlog'), task('ATL-9', 'Backlog')];
			board.tasks = listed();
			mocks.boardStages.mockResolvedValue({ stages: STAGES, overridden: false });
			mocks.listTasks.mockImplementation(async () => listed());
			mocks.getTask.mockImplementation(async (key: string) => ({
				task: task(key, 'In Progress'),
				children: key === 'ATL-1' ? [task('ATL-2', 'Done')] : [],
				events: []
			}));
			const lists = () => mocks.listTasks.mock.calls.length;
			const gets = () => mocks.getTask.mock.calls.length;
			const stop = startBoardPolling(60_000);
			const before = lists();
			const g = gets();
			const change = (key: string | null, id = `id-${key}`) => ({ entity: 'task', action: 'moved', id, key, project_id: null, at: '' });

			// A lone change (however many times it repeats) costs one GET of that task
			// and no re-list; the row takes what the daemon has.
			dispatch(change('ATL-9'));
			dispatch(change('ATL-9'));
			dispatch(change('ATL-9'));
			expect(gets()).toBe(g);
			await vi.advanceTimersByTimeAsync(200);
			expect(lists()).toBe(before);
			expect(gets()).toBe(g + 1);
			expect(mocks.getTask).toHaveBeenLastCalledWith('ATL-9');
			expect(board.tasks.map((t) => [t.key, t.stage])).toEqual([
				['ATL-1', 'Backlog'],
				['ATL-2', 'Backlog'],
				['ATL-9', 'In Progress']
			]);

			// A burst naming two tasks re-lists once instead.
			dispatch(change('ATL-1'));
			dispatch(change('ATL-2'));
			await vi.advanceTimersByTimeAsync(200);
			expect(lists()).toBe(before + 1);
			expect(gets()).toBe(g + 1);

			// A task the board does not hold (a create) and a change with no key re-list.
			dispatch(change('ATL-5'));
			await vi.advanceTimersByTimeAsync(200);
			expect(lists()).toBe(before + 2);
			dispatch(change(null, 'id-ATL-9'));
			await vi.advanceTimersByTimeAsync(200);
			expect(lists()).toBe(before + 3);
			expect(gets()).toBe(g + 1);

			// With ATL-1 open, a change to its subtask ATL-2 reloads the detail, which
			// already folds the subtask's row in, so that is the only request.
			openTask('ATL-1');
			await vi.advanceTimersByTimeAsync(0);
			const opened = gets();
			dispatch(change('ATL-2'));
			await vi.advanceTimersByTimeAsync(200);
			expect(lists()).toBe(before + 3);
			expect(gets()).toBe(opened + 1);
			expect(mocks.getTask).toHaveBeenLastCalledWith('ATL-1');
			expect(board.tasks.find((t) => t.key === 'ATL-2')?.stage).toBe('Done');

			// A change elsewhere patches its own row and leaves the detail alone.
			dispatch(change('ATL-9'));
			await vi.advanceTimersByTimeAsync(200);
			expect(gets()).toBe(opened + 2);
			expect(mocks.getTask).toHaveBeenLastCalledWith('ATL-9');

			// Dropped events mean a wholesale refresh.
			dispatch({ entity: 'lagged', action: 'lagged', id: null, key: null, project_id: null, at: '' });
			await vi.advanceTimersByTimeAsync(0);
			expect(lists()).toBe(before + 4);

			stop();
			closeTask();
			dispatch(change('ATL-9'));
			await vi.advanceTimersByTimeAsync(200);
			expect(lists()).toBe(before + 4);
			expect(gets()).toBe(opened + 2);
		} finally {
			vi.useRealTimers();
		}
	});

	it('re-lists instead of patching while a text filter is on, and drops a patched task that a hidden-done board no longer shows', async () => {
		vi.useFakeTimers();
		try {
			board.tasks = [task('ATL-1', 'Backlog'), task('ATL-2', 'Backlog')];
			mocks.boardStages.mockResolvedValue({ stages: STAGES, overridden: false });
			mocks.listTasks.mockResolvedValue([task('ATL-1', 'Backlog'), task('ATL-2', 'Backlog')]);
			mocks.getTask.mockImplementation(async (key: string) => ({ task: task(key, 'Done'), children: [], events: [] }));
			const lists = () => mocks.listTasks.mock.calls.length;
			const stop = startBoardPolling(60_000);
			const change = (key: string) => ({ entity: 'task', action: 'moved', id: `id-${key}`, key, project_id: null, at: '' });

			board.filters.query = 'widget';
			const before = lists();
			dispatch(change('ATL-1'));
			await vi.advanceTimersByTimeAsync(200);
			expect(lists()).toBe(before + 1);
			expect(mocks.getTask).not.toHaveBeenCalled();

			board.filters.query = '';
			board.filters.hideDone = true;
			dispatch(change('ATL-2'));
			await vi.advanceTimersByTimeAsync(200);
			expect(lists()).toBe(before + 1);
			expect(board.tasks.map((t) => t.key)).toEqual(['ATL-1']);

			stop();
		} finally {
			vi.useRealTimers();
			board.filters.hideDone = false;
		}
	});

	it('re-lists on the interval and on focus, and stops when told', async () => {
		vi.useFakeTimers();
		try {
			mocks.boardStages.mockResolvedValue({ stages: STAGES, overridden: false });
			mocks.listTasks.mockResolvedValue([]);
			const before = mocks.listTasks.mock.calls.length;

			const stop = startBoardPolling(1000);
			await vi.advanceTimersByTimeAsync(1000);
			expect(mocks.listTasks.mock.calls.length).toBe(before + 1);
			await vi.advanceTimersByTimeAsync(1000);
			expect(mocks.listTasks.mock.calls.length).toBe(before + 2);

			if (typeof window !== 'undefined') {
				window.dispatchEvent(new Event('focus'));
				await vi.advanceTimersByTimeAsync(0);
				expect(mocks.listTasks.mock.calls.length).toBe(before + 3);
			}

			stop();
			await vi.advanceTimersByTimeAsync(5000);
			expect(mocks.listTasks.mock.calls.length).toBe(typeof window !== 'undefined' ? before + 3 : before + 2);
		} finally {
			vi.useRealTimers();
		}
	});
});

describe('openTask history', () => {
	it('remembers the task a subtask was opened from, steps back to it once, and forgets on close', async () => {
		mocks.getTask.mockResolvedValue(null);
		closeTask();

		openTask('ATL-1');
		expect(board.detailHistory).toEqual([]);
		openTask('ATL-2');
		expect(board.selected).toBe('ATL-2');
		expect(board.detailHistory).toEqual(['ATL-1']);
		// Reopening the same task adds nothing.
		openTask('ATL-2');
		expect(board.detailHistory).toEqual(['ATL-1']);

		backTask();
		expect(board.selected).toBe('ATL-1');
		expect(board.detailHistory).toEqual([]);
		backTask();
		expect(board.selected).toBe('ATL-1');

		openTask('ATL-3');
		closeTask();
		expect(board.selected).toBeNull();
		expect(board.detailHistory).toEqual([]);
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

	it('still asks the daemon when the task is not in the filtered list', async () => {
		// The drawer outlives a task's place in `board.tasks`: a search that no longer
		// matches it empties the list while the drawer stays open.
		board.tasks = [];
		board.selected = 'ATL-1';
		mocks.moveTask.mockResolvedValue(task('ATL-1', 'Done'));
		mocks.getTask.mockResolvedValue({ task: task('ATL-1', 'Done'), children: [], events: [] });
		mocks.boardStages.mockResolvedValue({ stages: STAGES, overridden: false });
		mocks.listTasks.mockResolvedValue([task('ATL-1', 'Done')]);

		await move('ATL-1', 'Done');

		expect(mocks.moveTask).toHaveBeenCalledWith('ATL-1', 'Done', undefined);
		expect(mocks.listTasks).toHaveBeenCalledTimes(1);
		expect(board.tasks[0].stage).toBe('Done');
	});

	it('leaves a stage someone else set rather than rolling back over it', async () => {
		board.tasks = [task('ATL-1', 'Backlog')];
		let reject: (e: unknown) => void = () => {};
		mocks.moveTask.mockReturnValue(new Promise((_, r) => (reject = r)));

		const pending = move('ATL-1', 'Done');
		// A refresh lands mid-flight and brings a stage a second agent set.
		board.tasks = [task('ATL-1', 'In Progress')];
		reject(new ApiError('nope', 400));
		await pending;

		expect(board.tasks[0].stage).toBe('In Progress');
	});
});


// The detail's writes (ARCH-11): each one calls the api once and patches the row the
// daemon sends back into the list and into the open detail, so the card and the panel
// never disagree.
describe('writes from the detail', () => {
	function openDetail(children: Task[] = []) {
		board.tasks = [task('ATL-1', 'Backlog'), task('ATL-2', 'Backlog'), ...children];
		board.detail = { task: task('ATL-1', 'Backlog'), children, events: [] };
	}

	it('updateTask patches the list row and the open task with what the daemon returns', async () => {
		openDetail();
		const fresh = { ...task('ATL-1', 'Backlog'), title: 'Renamed' };
		mocks.updateTask.mockResolvedValue(fresh);

		const got = await updateTask('ATL-1', { title: 'Renamed' });

		expect(mocks.updateTask).toHaveBeenCalledTimes(1);
		expect(mocks.updateTask).toHaveBeenCalledWith('ATL-1', { title: 'Renamed' });
		expect(got).toEqual(fresh);
		expect(board.tasks[0].title).toBe('Renamed');
		expect(board.detail?.task.title).toBe('Renamed');
	});

	it('updateTask turns a 409 into ConflictError and leaves the list alone', async () => {
		openDetail();
		mocks.updateTask.mockRejectedValue(new ApiError('changed', 409));

		await expect(updateTask('ATL-1', { title: 'x' })).rejects.toBeInstanceOf(ConflictError);
		expect(board.tasks[0].title).toBe('ATL-1');
	});

	it('updateTask rethrows any other failure as it is', async () => {
		openDetail();
		const boom = new ApiError('nope', 400);
		mocks.updateTask.mockRejectedValue(boom);

		await expect(updateTask('ATL-1', { title: 'x' })).rejects.toBe(boom);
	});

	it('setBlockers and claim patch the row and a child of the open detail', async () => {
		openDetail([task('ATL-3', 'Backlog')]);
		mocks.setTaskBlockers.mockResolvedValue({ ...task('ATL-1', 'Backlog'), blocked_by: ['ATL-2'] });
		mocks.claimTask.mockResolvedValue({ ...task('ATL-3', 'In Progress'), assignee: 'codex' });

		await setBlockers('ATL-1', ['ATL-2']);
		await claim('ATL-3');

		expect(mocks.setTaskBlockers).toHaveBeenCalledWith('ATL-1', ['ATL-2']);
		expect(mocks.claimTask).toHaveBeenCalledWith('ATL-3');
		expect(board.tasks[0].blocked_by).toEqual(['ATL-2']);
		expect(board.detail?.task.blocked_by).toEqual(['ATL-2']);
		expect(board.tasks[2].stage).toBe('In Progress');
		expect(board.detail?.children[0].assignee).toBe('codex');
	});

	it('comment appends the event to the open detail and not to another task', async () => {
		openDetail();
		const ev = { id: 'e1', task_id: 'id-ATL-1', actor: 'desktop', kind: 'commented', body: 'hi', detail: null, created_at: '2026-09-03T10:00:00Z' };
		mocks.commentTask.mockResolvedValue(ev);

		await comment('ATL-1', 'hi');
		await comment('ATL-2', 'elsewhere');

		expect(mocks.commentTask).toHaveBeenNthCalledWith(1, 'ATL-1', 'hi');
		expect(board.detail?.events).toEqual([ev]);
	});

	it('removeTask drops the row and a child of the open detail', async () => {
		openDetail([task('ATL-3', 'Backlog')]);
		mocks.deleteTask.mockResolvedValue(undefined);

		await removeTask('ATL-3');

		expect(mocks.deleteTask).toHaveBeenCalledWith('ATL-3');
		expect(board.tasks.map((t) => t.key)).toEqual(['ATL-1', 'ATL-2']);
		expect(board.detail?.children).toEqual([]);
	});

	it('importFramework hands the arguments through and returns the report', async () => {
		const report = { created: 1, updated: 0, skipped: 2 };
		mocks.importFramework.mockResolvedValue(report);

		const got = await importFramework('p1', 'superpowers', 'tasks');

		expect(mocks.importFramework).toHaveBeenCalledWith('p1', 'superpowers', 'tasks');
		expect(got).toBe(report);
	});
});
