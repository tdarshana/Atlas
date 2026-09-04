// @vitest-environment jsdom
// Task 3 review, Minor 2: `documentRows([]) === []` proves the row shaping is right but
// never exercised the actual empty-state copy the tab renders. This mounts the real page
// against an empty `listFrameworks` answer and asserts the rendered text.

import { describe, expect, it, vi } from 'vitest';
import { render, waitFor } from '@testing-library/svelte';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({
		listFrameworks: async () => []
	}),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

vi.mock('$lib/stores/project.svelte', () => ({
	project: { current: { id: 'p1' } },
	setHeaderActions: vi.fn()
}));

vi.mock('$lib/stores/board.svelte', () => ({
	board: { filters: { projectId: null as string | null } },
	refresh: async () => {}
}));

import Page from './+page.svelte';

describe('Frameworks tab empty state', () => {
	it('names all four framework folders when nothing is detected', async () => {
		const { getByTestId } = render(Page);

		const empty = await waitFor(() => getByTestId('frameworks-empty'));
		expect(empty.textContent).toContain('No framework detected');
		expect(empty.textContent).toContain('docs/superpowers/specs');
		expect(empty.textContent).toContain('docs/superpowers/plans');
		expect(empty.textContent).toContain('.superpowers/sdd');
		expect(empty.textContent).toContain('openspec/specs');
		expect(empty.textContent).toContain('openspec/changes');
		expect(empty.textContent).toContain('.specify');
		expect(empty.textContent).toContain('.planning');
	});
});
