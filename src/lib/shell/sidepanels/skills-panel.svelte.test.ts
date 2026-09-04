// @vitest-environment jsdom
// The Skills side panel counts the same list the table draws, and clicking a source row
// sets the Source filter the route reads.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import type { SkillSource, SkillSummary } from '$lib/types';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({ listSkills: async () => ({ skills: [], warnings: [] }) }),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { skills } from '$lib/stores/skills.svelte';
import Skills from './Skills.svelte';

afterEach(cleanup);

function summary(name: string, source: SkillSource, plugin: string | null = null): SkillSummary {
	return {
		id: `${source}:${name}`,
		source,
		name,
		description: '',
		scope: 'global',
		project_id: null,
		path: null,
		plugin,
		editable: true,
		updated_at: null
	};
}

beforeEach(() => {
	skills.items = [
		summary('a', 'native'),
		summary('b', 'claude-user'),
		summary('c', 'claude-project'),
		summary('d', 'plugin', 'anthropics/example-skills'),
		summary('e', 'plugin', 'sm/rust')
	];
	skills.source = 'all';
	skills.loading = false;
});

describe('Skills side panel', () => {
	it('counts every source and every plugin', () => {
		render(Skills);
		expect(screen.getByText('Claude Code').closest('a, button')?.textContent).toContain('2');
		expect(screen.getByText('Native').closest('a, button')?.textContent).toContain('1');
		expect(screen.getByText('anthropics/example-skills')).toBeTruthy();
		expect(screen.getByText('sm/rust')).toBeTruthy();
	});

	it('sets the Source filter from a source row', async () => {
		render(Skills);
		await fireEvent.click(screen.getByText('Codex'));
		expect(skills.source).toBe('codex');
		await fireEvent.click(screen.getByText('All'));
		expect(skills.source).toBe('all');
	});

	it('lists no plugin rows when nothing contributes one', () => {
		skills.items = [summary('a', 'native')];
		render(Skills);
		expect(screen.queryByText('anthropics/example-skills')).toBeNull();
		expect(screen.queryByText('sm/rust')).toBeNull();
	});
});
