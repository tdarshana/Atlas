// @vitest-environment jsdom
// The dashboard shows the newest few memories, so it asks the daemon for that many
// rather than listing the whole active set and keeping the top of it.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, waitFor } from '@testing-library/svelte';

const mocks = vi.hoisted(() => ({
	listMemories: vi.fn(async () => []),
	listProjects: vi.fn(async () => []),
	listAgents: vi.fn(async () => []),
	boardStages: vi.fn(async () => ({ stages: [] })),
	taskCounts: vi.fn(async () => [])
}));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import Dashboard from './+page.svelte';

afterEach(() => cleanup());

describe('the dashboard', () => {
	it('asks for only the newest memories it shows', async () => {
		render(Dashboard);
		await waitFor(() => expect(mocks.listMemories).toHaveBeenCalledTimes(1));
		expect(mocks.listMemories).toHaveBeenCalledWith('active', undefined, undefined, { limit: 10 });
	});
});
