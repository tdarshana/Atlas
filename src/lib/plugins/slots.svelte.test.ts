// @vitest-environment jsdom
// The two rendered contribution slots: a `dashboard.card` per ref on the dashboard, and a
// `task.detail.panel` per ref under the task detail's tabs. The panel is also told which
// task is open, and told again when the board moves to another one.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, waitFor } from '@testing-library/svelte';
import type { Stage, Task, TaskDetail as TaskDetailType } from '$lib/types';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({
		listMemories: async () => [],
		listProjects: async () => [],
		listAgents: async () => [],
		boardStages: async () => ({ stages: [] }),
		taskCounts: async () => []
	}),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import TaskDetail from '$lib/components/board/TaskDetail.svelte';
import Dashboard from '../../routes/+page.svelte';
import { plugins } from './host.svelte';
import type { Contributes, Manifest, PluginInfo } from './types';

function plugin(id: string, name: string, contributes: Partial<Contributes>): PluginInfo {
	const manifest: Manifest = {
		id,
		name,
		version: '1.0.0',
		description: 'd',
		author: 'a',
		api: '>=1.0 <2',
		main: 'main.js',
		// The manifest validator refuses a section, a component or a tool without its
		// permission, so a fixture that contributes any of them has to ask for all three.
		permissions: ['ui.sections', 'ui.components', 'mcp.tools'],
		contributes: { sections: [], themes: [], components: [], commands: [], tools: [], ...contributes }
	};
	return {
		id,
		manifest,
		enabled: true,
		compatible: true,
		reason: null,
		dir: `/plugins/${id}`,
		granted: [...manifest.permissions]
	};
}

const HELLO = plugin('hello-world', 'Hello World', {
	components: [
		{ slot: 'dashboard.card', id: 'ready', view: 'ready-card' },
		{ slot: 'task.detail.panel', id: 'looking', view: 'task-panel' }
	]
});

const SECOND = plugin('second', 'Second', {
	components: [{ slot: 'dashboard.card', id: 'extra', view: 'extra-card' }]
});

const STAGES: Stage[] = [
	{ name: 'Backlog', done: false },
	{ name: 'Done', done: true }
];

function task(key: string): Task {
	return {
		id: `id-${key}`,
		key,
		project_id: null,
		seq: 1,
		title: `Title of ${key}`,
		description: '',
		stage: 'Backlog',
		kind: 'chore',
		priority: 'medium',
		assignee: null,
		labels: [],
		parent_id: null,
		parent_key: null,
		parent_title: null,
		created_by: 'desktop',
		created_at: '2026-09-04T10:00:00Z',
		updated_at: '2026-09-04T10:00:00Z',
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

function detail(key: string): TaskDetailType {
	return { task: task(key), children: [], events: [] };
}

function openDetail(d: TaskDetailType) {
	return render(TaskDetail, {
		props: {
			detail: d,
			stages: STAGES,
			loading: false,
			error: null,
			width: 340,
			mode: 'docked' as const,
			ontogglemode: () => {},
			onclose: () => {},
			onchanged: async () => {},
			onmove: () => {},
			ondeleted: async () => {},
			onresize: () => {}
		}
	});
}

beforeEach(() => {
	localStorage.clear();
	plugins.items = [];
	// Already loaded, so neither surface refetches and overwrites what a test put here.
	plugins.loaded = true;
	plugins.available = true;
});

afterEach(() => {
	cleanup();
	plugins.items = [];
	plugins.loaded = false;
});

describe('the dashboard.card slot', () => {
	it('renders one card per contributed component', async () => {
		plugins.items = [HELLO, SECOND];

		const { getByTestId } = render(Dashboard);

		await waitFor(() => getByTestId('plugin-card-hello-world-ready'));
		const card = getByTestId('plugin-card-hello-world-ready');
		expect(card.className).toContain('stat-card');
		expect(card.textContent).toContain('Hello World');
		expect(getByTestId('plugin-card-second-extra')).toBeTruthy();
	});

	it('renders no card at all when no plugin is installed', () => {
		const { queryByTestId } = render(Dashboard);

		expect(queryByTestId('plugin-card-hello-world-ready')).toBeNull();
		// The slot is not filled by the task panel component either.
		expect(queryByTestId('plugin-panel-hello-world-looking')).toBeNull();
	});
});

describe('the task.detail.panel slot', () => {
	it('renders one panel per contributed component, under the tabs', () => {
		plugins.items = [HELLO, SECOND];

		const { getByTestId, queryByTestId } = openDetail(detail('ATL-1'));

		const panel = getByTestId('plugin-panel-hello-world-looking');
		expect(panel.querySelector('h3')?.textContent).toBe('Hello World');
		// The panel comes after the tabs, which is where the brief puts it.
		const tabs = getByTestId('task-tabs');
		expect(tabs.compareDocumentPosition(panel) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
		// `second` fills the dashboard slot only, so it contributes no panel.
		expect(queryByTestId('plugin-panel-second-extra')).toBeNull();
	});

	it('renders nothing when the store is empty', () => {
		const { queryByTestId } = openDetail(detail('ATL-1'));

		expect(queryByTestId('plugin-panel-hello-world-looking')).toBeNull();
	});

	it('sends the open task to the frame, and sends it again when the task changes', async () => {
		plugins.items = [HELLO];
		// The frame is a real iframe on a scheme jsdom will not fetch, so the handshake is
		// driven by hand: say hello as the client script would, then read what came back.
		const posted: Record<string, unknown>[] = [];
		const frameWindow = { postMessage: (m: unknown) => void posted.push(m as Record<string, unknown>) };

		const { findByTestId, rerender } = openDetail(detail('ATL-1'));
		// The frame's src waits on `resolvePlatform`, so the iframe appears a tick later.
		const iframe = (await findByTestId('plugin-frame-hello-world')) as HTMLIFrameElement;
		Object.defineProperty(iframe, 'contentWindow', { value: frameWindow, configurable: true });

		window.dispatchEvent(
			new MessageEvent('message', { data: { type: 'atlas:hello' }, source: frameWindow as never })
		);

		await waitFor(() => expect(posted).toHaveLength(1));
		expect(posted[0]).toMatchObject({
			type: 'atlas:init',
			plugin: { id: 'hello-world', view: 'task-panel', slot: 'task.detail.panel' },
			context: { taskKey: 'ATL-1' }
		});

		await rerender({ detail: detail('ATL-2') });

		await waitFor(() =>
			expect(posted.at(-1)).toEqual({ type: 'atlas:context', context: { taskKey: 'ATL-2' } })
		);
	});
});
