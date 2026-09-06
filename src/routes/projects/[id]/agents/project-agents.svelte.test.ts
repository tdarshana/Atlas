// @vitest-environment jsdom
// The project Personas tab: the roster in position order with one default radio, and
// `Add from library…` writing the whole roster back with the picked persona appended.

import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import type { Persona, Project, RosterRow } from '$lib/types';

// jsdom has no top layer, so the picker dialog needs the same stub the MCP page tests use.
beforeAll(() => {
	HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement) {
		this.open = true;
	};
	HTMLDialogElement.prototype.close = function close(this: HTMLDialogElement) {
		this.open = false;
	};
});

const mocks = vi.hoisted(() => ({
	listAgents: vi.fn(),
	getProjectRoster: vi.fn(),
	setProjectRoster: vi.fn()
}));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

// `project.current` is a getter over the projects store, and the tab only ever reads it.
const headerActions = vi.hoisted(() => vi.fn());

vi.mock('$lib/stores/project.svelte', () => ({
	project: { current: { id: 'p-1', name: 'atlas', root_path: '/repo' } as unknown as Project },
	setHeaderActions: (...args: unknown[]) => headerActions(...args)
}));

import { personas } from '$lib/stores/personas.svelte';
import ProjectPersonasPage from './+page.svelte';

function persona(name: string): Persona {
	return {
		id: `id-${name}`,
		name,
		slug: name.toLowerCase(),
		role: `${name} role`,
		summary: '',
		instructions: '',
		skills: [],
		workflows: [],
		practices: [],
		mcp_servers: [],
		tools: [],
		access: { memory_write: 'allow', task_move: 'allow', workflow_trigger: 'allow' },
		models: {},
		tags: [],
		created_at: '2026-09-05T00:00:00Z',
		updated_at: '2026-09-05T00:00:00Z'
	};
}

function row(name: string, position: number, is_default = false): RosterRow {
	return {
		persona_id: `id-${name}`,
		name,
		slug: name.toLowerCase(),
		role: `${name} role`,
		summary: '',
		tags: [],
		is_default,
		position,
		project_id: 'p-1'
	};
}

beforeEach(() => {
	for (const fn of Object.values(mocks)) fn.mockReset();
	mocks.listAgents.mockResolvedValue([persona('Reviewer'), persona('Builder'), persona('Tester')]);
	mocks.getProjectRoster.mockResolvedValue([row('Reviewer', 0, true), row('Builder', 1)]);
	mocks.setProjectRoster.mockImplementation(async () => [
		row('Reviewer', 0, true),
		row('Builder', 1),
		row('Tester', 2)
	]);
	personas.items = [];
	personas.roster = [];
	personas.rosterProjectId = null;
});

afterEach(cleanup);

describe('the project Agents tab', () => {
	it('draws the roster in position order with the default radio on the default', async () => {
		render(ProjectPersonasPage);
		await waitFor(() => screen.getByTestId('roster-row-reviewer'));

		const rows = screen.getAllByTestId(/^roster-row-/).map((r) => r.getAttribute('data-testid'));
		expect(rows).toEqual(['roster-row-reviewer', 'roster-row-builder']);
		expect(mocks.getProjectRoster).toHaveBeenCalledWith('p-1');
		expect((screen.getByTestId('roster-default-reviewer') as HTMLInputElement).checked).toBe(true);
		expect((screen.getByTestId('roster-default-builder') as HTMLInputElement).checked).toBe(false);
		expect(screen.getByTestId('roster-sync-link').getAttribute('href')).toBe('#project-sync');
	});

	it('registers Add from library… as the tab header action, once', () => {
		render(ProjectPersonasPage);
		expect(headerActions.mock.calls.length).toBeGreaterThan(0);
		expect(typeof headerActions.mock.calls[0][0]).toBe('function');
		// The header owns the button; the body carries no second copy beside the hint.
		expect(screen.queryByTestId('roster-add')).toBeNull();
	});

	it('adds from the library by writing the whole roster with the pick appended', async () => {
		personas.roster = [];
		mocks.getProjectRoster.mockResolvedValue([]);
		render(ProjectPersonasPage);
		await waitFor(() => screen.getByTestId('roster-add-empty'));

		await fireEvent.click(screen.getByTestId('roster-add-empty'));
		await waitFor(() => screen.getByTestId('roster-pick-tester'));
		await fireEvent.click(screen.getByTestId('roster-pick-tester'));
		await waitFor(() => expect(mocks.setProjectRoster).toHaveBeenCalled());
		expect(mocks.setProjectRoster).toHaveBeenCalledWith('p-1', [
			expect.objectContaining({ persona_id: 'id-Tester', position: 0 })
		]);
	});

	it('moves a row down by swapping positions and writing the list', async () => {
		render(ProjectPersonasPage);
		await waitFor(() => screen.getByTestId('roster-row-reviewer'));

		await fireEvent.click(screen.getByTestId('roster-down-reviewer'));
		await waitFor(() => expect(mocks.setProjectRoster).toHaveBeenCalled());
		expect(mocks.setProjectRoster).toHaveBeenCalledWith('p-1', [
			{ persona_id: 'id-Builder', is_default: false, position: 0 },
			{ persona_id: 'id-Reviewer', is_default: true, position: 1 }
		]);
	});
});
