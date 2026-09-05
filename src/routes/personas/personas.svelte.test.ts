// @vitest-environment jsdom
// The Personas view: a row per library persona, a docked detail for the open one with
// the six-row model grid, and a Delete that asks before it writes.

import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import type { Persona } from '$lib/types';

// jsdom has no top layer, so the delete confirm dialog needs the same stub the MCP page tests use.
beforeAll(() => {
	HTMLDialogElement.prototype.showModal = function showModal(this: HTMLDialogElement) {
		this.open = true;
	};
	HTMLDialogElement.prototype.close = function close(this: HTMLDialogElement) {
		this.open = false;
	};
});

const mocks = vi.hoisted(() => ({
	listPersonas: vi.fn(),
	getPersona: vi.fn(),
	createPersona: vi.fn(),
	updatePersona: vi.fn(),
	deletePersona: vi.fn(),
	getProjectRoster: vi.fn(),
	listProjects: vi.fn(),
	listSkills: vi.fn(),
	listWorkflows: vi.fn(),
	listDocs: vi.fn(),
	listMcpServers: vi.fn()
}));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { personas } from '$lib/stores/personas.svelte';
import PersonasPage from './+page.svelte';

function persona(name: string, over: Partial<Persona> = {}): Persona {
	return {
		id: `id-${name}`,
		name,
		slug: name.toLowerCase(),
		role: `${name} role`,
		summary: `${name} summary`,
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
		updated_at: '2026-09-05T00:00:00Z',
		...over
	};
}

const library = [persona('Reviewer', { skills: ['s-1', 's-2'], workflows: ['Nightly'] }), persona('Builder')];

beforeEach(() => {
	for (const fn of Object.values(mocks)) fn.mockReset();
	mocks.listPersonas.mockResolvedValue(library);
	mocks.getPersona.mockImplementation(
		async (id: string) => library.find((p) => p.id === id || p.slug === id) ?? library[0]
	);
	mocks.deletePersona.mockResolvedValue(undefined);
	mocks.listProjects.mockResolvedValue([{ id: 'p-1', name: 'atlas' }]);
	mocks.getProjectRoster.mockResolvedValue([]);
	mocks.listSkills.mockResolvedValue({ skills: [], warnings: [] });
	mocks.listWorkflows.mockResolvedValue([]);
	mocks.listDocs.mockResolvedValue([]);
	mocks.listMcpServers.mockResolvedValue({ servers: [], warnings: [] });
	personas.items = [];
	personas.open = null;
	personas.selectedId = null;
	personas.rosters = [];
});

afterEach(cleanup);

describe('the Personas view', () => {
	it('lists a row per persona with its slug as the test id', async () => {
		render(PersonasPage);
		await waitFor(() => screen.getByTestId('persona-row-reviewer'));
		expect(screen.getByTestId('persona-row-builder')).toBeTruthy();
		expect(screen.getByTestId('persona-row-reviewer').textContent).toContain('Reviewer role');
	});

	it('opens the detail from a row and draws six model rows', async () => {
		render(PersonasPage);
		await waitFor(() => screen.getByTestId('persona-row-reviewer'));
		await fireEvent.click(screen.getByTestId('persona-row-reviewer'));

		await waitFor(() => screen.getByTestId('persona-detail'));
		expect(mocks.getPersona).toHaveBeenCalledWith('id-Reviewer');
		const models = screen.getAllByTestId(/^persona-model-/);
		expect(models.map((m) => m.getAttribute('data-testid'))).toEqual([
			'persona-model-plan',
			'persona-model-implement',
			'persona-model-review',
			'persona-model-test',
			'persona-model-document',
			'persona-model-default'
		]);
		expect(screen.getByTestId('persona-save')).toBeTruthy();
	});

	it('asks before deleting and only writes on confirm', async () => {
		render(PersonasPage);
		await waitFor(() => screen.getByTestId('persona-row-reviewer'));
		await fireEvent.click(screen.getByTestId('persona-row-reviewer'));
		await waitFor(() => screen.getByTestId('persona-delete'));

		await fireEvent.click(screen.getByTestId('persona-delete'));
		expect(mocks.deletePersona).not.toHaveBeenCalled();

		await waitFor(() => screen.getByTestId('persona-delete-confirm'));
		await fireEvent.click(screen.getByTestId('persona-delete-confirm'));
		await waitFor(() => expect(mocks.deletePersona).toHaveBeenCalledWith('id-Reviewer'));
	});
});
