// @vitest-environment jsdom
// The task detail's lower half: a Subtasks/Activity/Comments tab strip over the
// children list, the non-comment events and the comment events plus the composer.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';
import type { Stage, Task, TaskDetail as TaskDetailType, TaskEvent } from '$lib/types';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({}),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { DETAIL_TAB_KEY, setDetailTab } from '$lib/stores/board.svelte';
import TaskDetail from './TaskDetail.svelte';

afterEach(cleanup);

beforeEach(() => {
	localStorage.clear();
	setDetailTab('subtasks');
});

const STAGES: Stage[] = [
	{ name: 'Backlog', done: false },
	{ name: 'Done', done: true }
];

function task(key: string, stage = 'Backlog'): Task {
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

function event(id: string, kind: string, body: string): TaskEvent {
	return {
		id,
		task_id: 'id-ATL-1',
		actor: 'codex',
		kind,
		body,
		detail: null,
		created_at: '2026-09-03T10:00:00Z'
	};
}

function detail(): TaskDetailType {
	return {
		task: task('ATL-1'),
		children: [task('ATL-2', 'Backlog'), task('ATL-3', 'Done')],
		events: [event('e1', 'created', 'created ATL-1'), event('e2', 'moved', 'moved to Done'), event('e3', 'commented', 'looks good')]
	};
}

function open(d: TaskDetailType | null = detail()) {
	return render(TaskDetail, {
		props: {
			detail: d,
			stages: STAGES,
			loading: false,
			error: null,
			width: 340,
			mode: 'docked',
			ontogglemode: () => {},
			onclose: () => {},
			onchanged: async () => {},
			onmove: () => {},
			ondeleted: async () => {},
			onresize: () => {}
		}
	});
}

describe('TaskDetail tabs', () => {
	it('shows three tabs with the subtasks, non-comment and comment counts in a badge', () => {
		const { container } = open();

		const strip = container.querySelector('[data-testid="task-tabs"]');
		expect(strip).not.toBeNull();

		const subtasksTab = container.querySelector('[data-testid="task-tab-subtasks"]');
		expect(subtasksTab?.textContent).toContain('Subtasks');
		expect(subtasksTab?.querySelector('.dbm-badge')?.textContent?.trim()).toBe('2');

		const activityTab = container.querySelector('[data-testid="task-tab-activity"]');
		expect(activityTab?.textContent).toContain('Activity');
		expect(activityTab?.querySelector('.dbm-badge')?.textContent?.trim()).toBe('2');

		const commentsTab = container.querySelector('[data-testid="task-tab-comments"]');
		expect(commentsTab?.textContent).toContain('Comments');
		expect(commentsTab?.querySelector('.dbm-badge')?.textContent?.trim()).toBe('1');
	});

	it('shows the children and not the events with Subtasks selected by default', () => {
		const { container } = open();

		expect(container.querySelector('[data-testid="task-children"]')).not.toBeNull();
		expect(container.querySelector('[data-testid="task-events"]')).toBeNull();
		expect(container.querySelectorAll('[data-testid="task-children"] li')).toHaveLength(2);
	});

	it('selecting Activity shows the two non-comment events and no textarea', async () => {
		const { container } = open();

		await fireEvent.click(container.querySelector('[data-testid="task-tab-activity"]')!);

		const events = container.querySelectorAll('[data-testid="task-events"] li');
		expect(events).toHaveLength(2);
		expect(container.querySelector('[data-testid="task-comment"]')).toBeNull();
	});

	it('selecting Comments shows the one comment event and the textarea', async () => {
		const { container } = open();

		await fireEvent.click(container.querySelector('[data-testid="task-tab-comments"]')!);

		const events = container.querySelectorAll('[data-testid="task-events"] li');
		expect(events).toHaveLength(1);
		expect(container.querySelector('[data-testid="task-comment"]')).not.toBeNull();
		expect(container.querySelector('[data-testid="task-comment-send"]')).not.toBeNull();
	});

	it('persists the selected tab under atlas.board.detail.tab', async () => {
		const { container } = open();

		await fireEvent.click(container.querySelector('[data-testid="task-tab-comments"]')!);

		expect(localStorage.getItem(DETAIL_TAB_KEY)).toBe('comments');
	});
});
