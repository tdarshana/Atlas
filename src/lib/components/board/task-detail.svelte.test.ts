// @vitest-environment jsdom
// The task detail's lower half: a Subtasks/Activity/Comments tab strip over the
// children list, the non-comment events and the comment events plus the composer.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, waitFor } from '@testing-library/svelte';
import type { Stage, Task, TaskDetail as TaskDetailType, TaskEvent } from '$lib/types';

const mocks = vi.hoisted(() => ({ updateTask: vi.fn() }));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { DETAIL_TAB_KEY, setDetailTab } from '$lib/stores/board.svelte';
import { clear as clearToasts, toasts } from '$lib/ui/toasts.svelte';
import TaskDetail from './TaskDetail.svelte';

afterEach(cleanup);

beforeEach(() => {
	localStorage.clear();
	setDetailTab('subtasks');
	mocks.updateTask.mockReset();
	mocks.updateTask.mockResolvedValue({});
	clearToasts();
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

function open(d: TaskDetailType | null = detail(), overrides: Record<string, unknown> = {}) {
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
			onresize: () => {},
			...overrides
		}
	});
}

describe('TaskDetail parent and back', () => {
	it('names the parent above a subtask title and opens it from that line', async () => {
		const opened: string[] = [];
		const d = detail();
		d.task = { ...d.task, parent_id: 'id-ATL-0', parent_key: 'ATL-0', parent_title: 'The parent' };
		const { container } = open(d, { onopen: (key: string) => opened.push(key) });

		const line = container.querySelector('[data-testid="task-detail-parent"]')!;
		expect(line.textContent).toContain('ATL-0');
		expect(line.textContent).toContain('The parent');
		await fireEvent.click(line);
		expect(opened).toEqual(['ATL-0']);
	});

	it('draws no parent line on a top-level task', () => {
		const { container } = open();
		expect(container.querySelector('[data-testid="task-detail-parent"]')).toBeNull();
	});

	it('shows the back button only with a task to go back to, and it calls onback', async () => {
		const none = open();
		expect(none.container.querySelector('[data-testid="task-detail-back"]')).toBeNull();
		none.unmount();

		let backs = 0;
		const { container } = open(detail(), { backKey: 'ATL-0', onback: () => backs++ });
		const back = container.querySelector('[data-testid="task-detail-back"]')!;
		expect(back.getAttribute('aria-label')).toBe('Back to ATL-0');
		await fireEvent.click(back);
		expect(backs).toBe(1);
	});
});

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

describe('TaskDetail title and description edit in place', () => {
	it('shows the title and description as text with edit pencils, no editor mounted', () => {
		const { container } = open();

		expect(container.querySelector('[data-testid="task-title-text"]')?.textContent).toBe(
			'Title of ATL-1'
		);
		expect(container.querySelector('[data-testid="task-title-edit"]')).not.toBeNull();
		expect(container.querySelector('[data-testid="task-title"]')).toBeNull();

		expect(container.querySelector('[data-testid="task-description-edit"]')).not.toBeNull();
		expect(container.querySelector('[data-testid="task-description"]')).toBeNull();
		expect(
			container.querySelector('[data-testid="task-description-text"]')?.textContent
		).toContain('No description');
	});

	it('gives the description display box the textarea\'s own inner padding', () => {
		const { container } = open();

		const wrapper = container.querySelector('[data-testid="task-description-text"]');
		expect(wrapper?.classList.contains('description-display')).toBe(true);

		const body = wrapper?.querySelector('.body');
		expect(body).not.toBeNull();
		const style = getComputedStyle(body as Element);
		expect(style.paddingTop).toBe('6px');
		expect(style.paddingLeft).toBe('8px');
	});

	it('keeps the panel open when Escape cancels the title editor', async () => {
		const onclose = vi.fn();
		const { container } = open(detail(), { onclose });

		await fireEvent.click(container.querySelector('[data-testid="task-title-edit"]')!);
		const editor = container.querySelector<HTMLTextAreaElement>('[data-testid="task-title"]')!;

		await fireEvent.keyDown(editor, { key: 'Escape' });

		expect(container.querySelector('[data-testid="task-detail"]')).not.toBeNull();
		expect(onclose).not.toHaveBeenCalled();
		expect(container.querySelector('[data-testid="task-title"]')).toBeNull();
	});

	it('clicking the title text enters edit mode and Enter saves through the API with only { title }', async () => {
		const onchanged = vi.fn(async () => {});
		const { container } = open(detail(), { onchanged });

		await fireEvent.click(container.querySelector('[data-testid="task-title-text"]')!);

		const editor = container.querySelector<HTMLTextAreaElement>('[data-testid="task-title"]');
		expect(editor).not.toBeNull();
		expect(editor!.value).toBe('Title of ATL-1');

		await fireEvent.input(editor!, { target: { value: 'Renamed title' } });
		await fireEvent.keyDown(editor!, { key: 'Enter' });

		await waitFor(() => expect(onchanged).toHaveBeenCalled());
		expect(mocks.updateTask).toHaveBeenCalledTimes(1);
		expect(mocks.updateTask).toHaveBeenCalledWith('ATL-1', { title: 'Renamed title' });
		expect(container.querySelector('[data-testid="task-title"]')).toBeNull();
		expect(container.querySelector('[data-testid="task-title-text"]')?.textContent).toBe(
			'Renamed title'
		);
		expect(toasts.some((t) => t.kind === 'success')).toBe(true);
	});

	it('Escape restores the old title and does not call the API', async () => {
		const { container } = open();

		await fireEvent.click(container.querySelector('[data-testid="task-title-edit"]')!);
		const editor = container.querySelector<HTMLTextAreaElement>('[data-testid="task-title"]')!;

		await fireEvent.input(editor, { target: { value: 'Thrown away' } });
		await fireEvent.keyDown(editor, { key: 'Escape' });

		expect(container.querySelector('[data-testid="task-title"]')).toBeNull();
		expect(container.querySelector('[data-testid="task-title-text"]')?.textContent).toBe(
			'Title of ATL-1'
		);
		expect(mocks.updateTask).not.toHaveBeenCalled();
	});

	it('refuses an empty title inline and keeps the editor open', async () => {
		const { container } = open();

		await fireEvent.click(container.querySelector('[data-testid="task-title-edit"]')!);
		const editor = container.querySelector<HTMLTextAreaElement>('[data-testid="task-title"]')!;

		await fireEvent.input(editor, { target: { value: '   ' } });
		await fireEvent.keyDown(editor, { key: 'Enter' });

		expect(mocks.updateTask).not.toHaveBeenCalled();
		expect(container.querySelector('[data-testid="task-title"]')).not.toBeNull();
		expect(container.textContent).toContain('Title cannot be empty');
	});

	it('clicking the description enters edit mode and Mod+Enter saves { description }', async () => {
		const onchanged = vi.fn(async () => {});
		const { container } = open(detail(), { onchanged });

		await fireEvent.click(container.querySelector('[data-testid="task-description-edit"]')!);
		const editor = container.querySelector<HTMLTextAreaElement>(
			'[data-testid="task-description"]'
		)!;
		expect(editor).not.toBeNull();

		await fireEvent.input(editor, { target: { value: 'Some **details**.' } });
		await fireEvent.keyDown(editor, { key: 'Enter', ctrlKey: true });

		await waitFor(() => expect(onchanged).toHaveBeenCalled());
		expect(mocks.updateTask).toHaveBeenCalledTimes(1);
		expect(mocks.updateTask).toHaveBeenCalledWith('ATL-1', {
			description: 'Some **details**.'
		});
		expect(container.querySelector('[data-testid="task-description"]')).toBeNull();
	});

	it('the description editor has no max-height and hides its overflow instead of scrolling', async () => {
		const { container } = open();

		await fireEvent.click(container.querySelector('[data-testid="task-description-edit"]')!);
		const editor = container.querySelector<HTMLTextAreaElement>(
			'[data-testid="task-description"]'
		)!;

		const style = getComputedStyle(editor);
		expect(style.overflow).toBe('hidden');
		expect(style.maxHeight === '' || style.maxHeight === 'none').toBe(true);
	});

	it('switching tasks closes an open editor and shows the new task', async () => {
		const { container, rerender } = open(detail());

		await fireEvent.click(container.querySelector('[data-testid="task-title-edit"]')!);
		expect(container.querySelector('[data-testid="task-title"]')).not.toBeNull();

		const other: TaskDetailType = { task: task('ATL-9'), children: [], events: [] };
		await rerender({
			detail: other,
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
		});

		expect(container.querySelector('[data-testid="task-title"]')).toBeNull();
		expect(container.querySelector('[data-testid="task-title-text"]')?.textContent).toBe(
			'Title of ATL-9'
		);
	});

	it('focuses the title editor with the caret at the end when the pencil is clicked', async () => {
		const { container } = open();

		await fireEvent.click(container.querySelector('[data-testid="task-title-edit"]')!);
		const editor = container.querySelector<HTMLTextAreaElement>('[data-testid="task-title"]')!;

		expect(document.activeElement).toBe(editor);
		expect(editor.selectionStart).toBe(editor.value.length);
		expect(editor.selectionEnd).toBe(editor.value.length);
	});

	it('focuses the description editor with the caret at the end when the pencil is clicked', async () => {
		const d: TaskDetailType = {
			task: { ...task('ATL-1'), description: 'Some existing details.' },
			children: [],
			events: []
		};
		const { container } = open(d);

		await fireEvent.click(container.querySelector('[data-testid="task-description-edit"]')!);
		const editor = container.querySelector<HTMLTextAreaElement>(
			'[data-testid="task-description"]'
		)!;

		expect(document.activeElement).toBe(editor);
		expect(editor.selectionStart).toBe(editor.value.length);
		expect(editor.selectionEnd).toBe(editor.value.length);
	});

	it('a link inside the description opens instead of entering edit mode', async () => {
		const d: TaskDetailType = {
			task: { ...task('ATL-1'), description: 'See [the docs](https://example.com/docs).' },
			children: [],
			events: []
		};
		const openSpy = vi.spyOn(window, 'open').mockImplementation(() => null);
		const { container } = open(d);

		const link = container.querySelector<HTMLAnchorElement>(
			'[data-testid="task-description-text"] a[href]'
		);
		expect(link).not.toBeNull();

		await fireEvent.click(link!);

		expect(openSpy).toHaveBeenCalledWith(
			'https://example.com/docs',
			'_blank',
			'noopener,noreferrer'
		);
		expect(container.querySelector('[data-testid="task-description"]')).toBeNull();

		openSpy.mockRestore();
	});
});
