// @vitest-environment jsdom
// The board's geometry: what a lane or the docked detail may be dragged to, where those
// widths are kept, and which lanes the column filter leaves open.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Stage, Task } from '$lib/types';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({}),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import {
	board,
	clampDetail,
	clampLane,
	deriveColumns,
	DETAIL_DEFAULT,
	DETAIL_KEY,
	DETAIL_MAX,
	DETAIL_MIN,
	LANE_DEFAULT,
	LANE_MAX,
	LANE_MIN,
	laneKey,
	laneWidth,
	loadCollapsedLanes,
	loadDetailWidth,
	loadLaneWidths,
	loadLayout,
	pruneCollapsedLanes,
	pruneLaneWidths,
	saveCollapsedLanes,
	saveDetailWidth,
	saveLaneWidths,
	setDetailWidth,
	setLaneWidth,
	toggleLaneCollapsed,
	visibleLanes
} from './board.svelte';

const STAGES: Stage[] = [
	{ name: 'Backlog', done: false },
	{ name: 'Testing', done: false },
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
		open_blockers: 0,
		ready: true,
		blocked_reason: null,
		subtasks_total: 0,
		subtasks_done: 0
	};
}

beforeEach(() => {
	localStorage.clear();
	board.filters.projectId = null;
	board.laneWidths = {};
	board.collapsedLanes = [];
	board.detailWidth = DETAIL_DEFAULT;
});

describe('clampLane', () => {
	it('holds a lane inside the design range and rounds to whole pixels', () => {
		expect(clampLane(340.4)).toBe(340);
		expect(clampLane(10)).toBe(LANE_MIN);
		expect(clampLane(9000)).toBe(LANE_MAX);
	});

	it('falls back to the minimum for a width that is not a number', () => {
		expect(clampLane(Number.NaN)).toBe(LANE_MIN);
	});
});

describe('clampDetail', () => {
	it('holds the docked panel between 280 and 560', () => {
		expect(clampDetail(400)).toBe(400);
		expect(clampDetail(100)).toBe(DETAIL_MIN);
		expect(clampDetail(700)).toBe(DETAIL_MAX);
	});
});

describe('lane width persistence', () => {
	it('keys the widths by project, so two boards keep their own arrangement', () => {
		expect(laneKey(null)).toBe('atlas.board.global.lanes');
		expect(laneKey('p1')).toBe('atlas.board.p1.lanes');

		saveLaneWidths('p1', { Backlog: 420 });
		saveLaneWidths(null, { Backlog: 260 });

		expect(loadLaneWidths('p1')).toEqual({ Backlog: 420 });
		expect(loadLaneWidths(null)).toEqual({ Backlog: 260 });
	});

	it('clamps what it reads back and drops entries that are not widths', () => {
		localStorage.setItem(
			laneKey('p1'),
			JSON.stringify({ Backlog: 9000, Testing: 'wide', Done: 300 })
		);

		expect(loadLaneWidths('p1')).toEqual({ Backlog: LANE_MAX, Done: 300 });
	});

	it('reads nothing from a missing, unparseable or wrongly shaped entry', () => {
		expect(loadLaneWidths('p1')).toEqual({});
		localStorage.setItem(laneKey('p1'), 'not json');
		expect(loadLaneWidths('p1')).toEqual({});
		localStorage.setItem(laneKey('p1'), '[340]');
		expect(loadLaneWidths('p1')).toEqual({});
	});

	it('writes a dragged lane through to storage under the open board', () => {
		board.filters.projectId = 'p1';
		setLaneWidth('Backlog', 9000);

		expect(board.laneWidths).toEqual({ Backlog: LANE_MAX });
		expect(JSON.parse(localStorage.getItem(laneKey('p1'))!)).toEqual({ Backlog: LANE_MAX });
	});

	it('gives an undragged lane the default width', () => {
		const widths = { Backlog: 500 };
		expect(laneWidth(widths, 'Backlog')).toBe(500);
		expect(laneWidth(widths, 'Testing')).toBe(LANE_DEFAULT);
	});
});

describe('pruneLaneWidths', () => {
	it('drops the width of a stage the board no longer has', () => {
		expect(pruneLaneWidths({ Backlog: 300, Retired: 500 }, STAGES)).toEqual({ Backlog: 300 });
	});

	it('keeps every live stage, so nothing is lost to a reload', () => {
		const widths = { Backlog: 300, Testing: 400 };
		expect(pruneLaneWidths(widths, STAGES)).toEqual(widths);
	});
});

describe('collapsed lane persistence', () => {
	it('folds a lane with its own button and remembers it per board', () => {
		board.filters.projectId = 'p1';

		toggleLaneCollapsed('Testing');
		expect(board.collapsedLanes).toEqual(['Testing']);
		expect(loadCollapsedLanes('p1')).toEqual(['Testing']);

		toggleLaneCollapsed('Backlog');
		expect(board.collapsedLanes).toEqual(['Testing', 'Backlog']);
		expect(loadCollapsedLanes('p1')).toEqual(['Testing', 'Backlog']);
	});

	it('opens a folded lane again on a second toggle', () => {
		board.filters.projectId = 'p1';
		toggleLaneCollapsed('Testing');
		toggleLaneCollapsed('Testing');

		expect(board.collapsedLanes).toEqual([]);
		expect(loadCollapsedLanes('p1')).toEqual([]);
	});

	it('keeps two boards separate, the same way lane widths do', () => {
		saveCollapsedLanes('p1', ['Testing']);
		saveCollapsedLanes('p2', ['Backlog']);

		expect(loadCollapsedLanes('p1')).toEqual(['Testing']);
		expect(loadCollapsedLanes('p2')).toEqual(['Backlog']);
	});

	it('reads nothing from a missing, unparseable or wrongly shaped entry', () => {
		expect(loadCollapsedLanes('p1')).toEqual([]);
		localStorage.setItem('atlas.board.p1.collapsed', 'not json');
		expect(loadCollapsedLanes('p1')).toEqual([]);
		localStorage.setItem('atlas.board.p1.collapsed', JSON.stringify({ not: 'an array' }));
		expect(loadCollapsedLanes('p1')).toEqual([]);
		localStorage.setItem('atlas.board.p1.collapsed', JSON.stringify(['Testing', 42]));
		expect(loadCollapsedLanes('p1')).toEqual(['Testing']);
	});
});

describe('pruneCollapsedLanes', () => {
	it('drops a folded stage the board no longer has', () => {
		expect(pruneCollapsedLanes(['Backlog', 'Retired'], STAGES)).toEqual(['Backlog']);
	});

	it('keeps every live stage folded, so nothing reopens on a reload', () => {
		expect(pruneCollapsedLanes(['Backlog', 'Testing'], STAGES)).toEqual(['Backlog', 'Testing']);
	});
});

describe('detail width persistence', () => {
	it('round-trips a width through one shared key', () => {
		saveDetailWidth(500);
		expect(localStorage.getItem(DETAIL_KEY)).toBe('500');
		expect(loadDetailWidth()).toBe(500);
	});

	it('clamps on the way in and out, and defaults when there is nothing to read', () => {
		expect(loadDetailWidth()).toBe(DETAIL_DEFAULT);
		saveDetailWidth(9000);
		expect(loadDetailWidth()).toBe(DETAIL_MAX);
		localStorage.setItem(DETAIL_KEY, 'wide');
		expect(loadDetailWidth()).toBe(DETAIL_DEFAULT);
	});

	it('writes a dragged panel through to the store and storage', () => {
		setDetailWidth(100);
		expect(board.detailWidth).toBe(DETAIL_MIN);
		expect(localStorage.getItem(DETAIL_KEY)).toBe(String(DETAIL_MIN));
	});
});

describe('loadLayout', () => {
	it('seeds the store from the board being opened', () => {
		saveLaneWidths('p2', { Backlog: 240 });
		saveDetailWidth(420);

		loadLayout('p2');

		expect(board.laneWidths).toEqual({ Backlog: 240 });
		expect(board.detailWidth).toBe(420);
	});
});

describe('visibleLanes', () => {
	const columns = deriveColumns(STAGES, [task('ATL-1', 'Testing')]);

	it('leaves every lane open when no column is chosen', () => {
		expect(visibleLanes(columns, null).map((l) => l.collapsed)).toEqual([false, false, false]);
	});

	it('folds every lane but the chosen one, keeping them all on the board', () => {
		const lanes = visibleLanes(columns, 'Testing');

		expect(lanes.map((l) => l.column.stage.name)).toEqual(['Backlog', 'Testing', 'Done']);
		expect(lanes.map((l) => l.collapsed)).toEqual([true, false, true]);
	});

	it('ignores a filter naming a column this board does not have', () => {
		expect(visibleLanes(columns, 'Retired').every((l) => !l.collapsed)).toBe(true);
	});

	it('marks a lane folded when its stage is in the folded set', () => {
		const lanes = visibleLanes(columns, null, ['Testing']);
		expect(lanes.map((l) => l.folded)).toEqual([false, true, false]);
	});

	it('leaves every lane unfolded when the folded set is empty', () => {
		expect(visibleLanes(columns, null, []).every((l) => !l.folded)).toBe(true);
	});

	it('keeps folded and collapsed independent: a filtered-away lane can also be folded', () => {
		const lanes = visibleLanes(columns, 'Testing', ['Backlog']);
		expect(lanes.map((l) => ({ collapsed: l.collapsed, folded: l.folded }))).toEqual([
			{ collapsed: true, folded: true },
			{ collapsed: false, folded: false },
			{ collapsed: true, folded: false }
		]);
	});
});
