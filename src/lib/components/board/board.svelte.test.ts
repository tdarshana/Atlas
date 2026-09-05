// @vitest-environment jsdom
// What the strip draws: a lane per stage plus the slot that adds one, a header that
// counts what its lane holds, and the card's priority dot.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render } from '@testing-library/svelte';
import type { Stage, Task } from '$lib/types';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({}),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { deriveColumns, visibleLanes } from '$lib/stores/board.svelte';
import LaneStrip from './LaneStrip.svelte';
import TaskCard from './TaskCard.svelte';
import { priorityTone } from './card';

afterEach(cleanup);

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
		title: `Title of ${key}`,
		description: '',
		stage,
		kind: 'chore',
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

function strip(stage: string | null = null) {
	const tasks = [task('ATL-1', 'Backlog'), task('ATL-2', 'Backlog'), task('ATL-3', 'Testing')];
	return render(LaneStrip, {
		props: {
			lanes: visibleLanes(deriveColumns(STAGES, tasks), stage),
			widths: { Backlog: 420 },
			// The shape the page passes, so the test renders the Select production builds.
			stageOptions: STAGES.map((s) => ({ value: s.name, label: s.name })),
			selected: null,
			onopen: () => {},
			onmove: () => {},
			onresize: () => {},
			onexpand: () => {},
			ontoggle: () => {},
			onaddcolumn: () => {}
		}
	});
}

describe('LaneStrip', () => {
	it('draws one lane per stage and the slot that adds another', () => {
		const { container } = strip();

		expect(container.querySelectorAll('[data-testid^="board-column-"]')).toHaveLength(
			STAGES.length
		);
		expect(container.querySelector('[data-testid="board-add-column"]')?.textContent).toContain(
			'Add column'
		);
	});

	it('names each lane and counts what it holds', () => {
		const { container } = strip();

		const header = container.querySelector('[data-testid="board-column-Backlog"] h2');
		expect(header?.textContent).toBe('Backlog');
		expect(
			container.querySelector('[data-testid="board-count-Backlog"]')?.textContent?.trim()
		).toBe('2');
		expect(
			container.querySelector('[data-testid="board-count-Testing"]')?.textContent?.trim()
		).toBe('1');
	});

	it('gives a dragged lane its width and the rest the default', () => {
		const { container } = strip();

		const backlog = container.querySelector<HTMLElement>('[data-testid="board-column-Backlog"]');
		const done = container.querySelector<HTMLElement>('[data-testid="board-column-Done"]');
		expect(backlog?.style.getPropertyValue('--lane-w')).toBe('420px');
		expect(done?.style.getPropertyValue('--lane-w')).toBe('340px');
	});

	it('opens a card for every task in a lane', () => {
		const { container } = strip();

		expect(container.querySelector('[data-testid="task-open-ATL-3"]')?.textContent).toContain(
			'Title of ATL-3'
		);
	});

	it('folds the other lanes to a header when a column is filtered, with a way back', () => {
		const { container } = strip('Testing');

		expect(container.querySelector('[data-testid="task-open-ATL-1"]')).toBeNull();
		expect(container.querySelector('[data-testid="task-open-ATL-3"]')).not.toBeNull();
		expect(container.querySelectorAll('[data-testid="board-show-all"]')).toHaveLength(2);
	});

	it('keeps a folded lane 44px wide and still says which stage it is', () => {
		const { container } = strip('Testing');

		const backlog = container.querySelector<HTMLElement>('[data-testid="board-column-Backlog"]');
		expect(backlog?.style.getPropertyValue('--lane-w')).toBe('44px');
		expect(backlog?.querySelector('h2')?.textContent).toBe('Backlog');
	});
});

describe('TaskCard', () => {
	function card(overrides: Partial<Task>) {
		return render(TaskCard, {
			props: {
				task: { ...task('ATL-1', 'Backlog'), ...overrides },
				stageOptions: STAGES.map((s) => ({ value: s.name, label: s.name })),
				selected: false,
				onopen: () => {},
				onmove: () => {}
			}
		});
	}

	it('shows a done/total chip once the task has subtasks', () => {
		const { container } = card({ subtasks_total: 3, subtasks_done: 1 });
		expect(container.querySelector('[data-testid="task-open-ATL-1"]')?.textContent).toContain('1/3');
	});

	it('shows no chip for a task with no subtasks', () => {
		const { container } = card({});
		expect(container.querySelector('[data-testid="task-open-ATL-1"]')?.textContent).not.toContain('/');
	});

	it('draws a subtask with its parent named above the title, and that line opens the parent', async () => {
		const opened: string[] = [];
		const { container } = render(TaskCard, {
			props: {
				task: { ...task('ATL-9', 'Backlog'), parent_id: 'id-ATL-1', parent_key: 'ATL-1', parent_title: 'Ship the widget' },
				stageOptions: [],
				selected: false,
				onopen: (key: string) => opened.push(key),
				onmove: () => {}
			}
		});
		const card = container.querySelector('[data-testid="task-open-ATL-9"]')!;
		expect(card.classList.contains('subtask')).toBe(true);
		const parent = container.querySelector('[data-testid="task-parent-ATL-9"]')!;
		expect(parent.textContent).toContain('ATL-1');
		expect(parent.textContent).toContain('Ship the widget');

		(parent as HTMLElement).click();
		expect(opened).toEqual(['ATL-1']);
		(card as HTMLElement).click();
		expect(opened).toEqual(['ATL-1', 'ATL-9']);
	});

	it('draws no parent line on a top-level task', () => {
		const { container } = card({});
		expect(container.querySelector('[data-testid="task-parent-ATL-1"]')).toBeNull();
		expect(container.querySelector('[data-testid="task-open-ATL-1"]')?.classList.contains('subtask')).toBe(false);
	});
});

describe('priorityTone', () => {
	it('gives each priority its own dot, and an unknown one the quietest', () => {
		expect(priorityTone('urgent')).toBe('var(--danger)');
		expect(priorityTone('high')).toBe('var(--warning)');
		expect(priorityTone('medium')).toBe('var(--text-tertiary)');
		expect(priorityTone('low')).toBe('var(--border-default)');
		expect(priorityTone('whenever')).toBe('var(--border-default)');
	});
});
