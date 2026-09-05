// @vitest-environment jsdom
// The personas store round-trips the library and a project's roster through the api,
// and a `persona` change on the stream reloads the list once per burst.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Persona, RosterRow } from '$lib/types';

const mocks = vi.hoisted(() => ({
	listPersonas: vi.fn(),
	getPersona: vi.fn(),
	createPersona: vi.fn(),
	updatePersona: vi.fn(),
	deletePersona: vi.fn(),
	getProjectRoster: vi.fn(),
	setProjectRoster: vi.fn(),
	listProjects: vi.fn()
}));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import { disconnectChanges, dispatch } from './changes.svelte';
import {
	closePersona,
	followPersonaChanges,
	loadPersonas,
	loadRoster,
	loadUsage,
	openPersona,
	PERSONA_CHANGE_DEBOUNCE_MS,
	personaRole,
	personas,
	projectsUsing,
	removePersona,
	savePersona,
	saveRoster
} from './personas.svelte';

function persona(name: string, over: Partial<Persona> = {}): Persona {
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
		updated_at: '2026-09-05T00:00:00Z',
		...over
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

const reviewer = persona('Reviewer');
const builder = persona('Builder');

beforeEach(() => {
	for (const fn of Object.values(mocks)) fn.mockReset();
	mocks.listPersonas.mockResolvedValue([reviewer, builder]);
	mocks.getPersona.mockResolvedValue(reviewer);
	mocks.createPersona.mockResolvedValue(builder);
	mocks.updatePersona.mockResolvedValue({ ...reviewer, role: 'Careful reviewer' });
	mocks.deletePersona.mockResolvedValue(undefined);
	mocks.getProjectRoster.mockResolvedValue([row('Reviewer', 0, true), row('Builder', 1)]);
	mocks.setProjectRoster.mockResolvedValue([row('Reviewer', 0, true), row('Builder', 1)]);
	mocks.listProjects.mockResolvedValue([
		{ id: 'p-1', name: 'atlas' },
		{ id: 'p-2', name: 'other' }
	]);
	personas.rosters = [];
	personas.items = [];
	personas.open = null;
	personas.roster = [];
	personas.rosterProjectId = null;
	personas.error = null;
});

afterEach(() => {
	disconnectChanges();
	vi.useRealTimers();
});

describe('loadPersonas and openPersona', () => {
	it('lists the library and clears the error', async () => {
		personas.error = 'stale';
		await loadPersonas();
		expect(mocks.listPersonas).toHaveBeenCalledTimes(1);
		expect(personas.items.map((p) => p.name)).toEqual(['Reviewer', 'Builder']);
		expect(personas.error).toBeNull();
		expect(personas.loading).toBe(false);
	});

	it('keeps the error message and an empty list when the daemon fails', async () => {
		mocks.listPersonas.mockRejectedValue(new Error('down'));
		await loadPersonas();
		expect(personas.items).toEqual([]);
		expect(personas.error).toBe('down');
	});

	it('opens one persona by id or slug and closes it again', async () => {
		await openPersona('reviewer');
		expect(mocks.getPersona).toHaveBeenCalledWith('reviewer');
		expect(personas.open?.id).toBe('id-Reviewer');
		expect(personas.selectedId).toBe('id-Reviewer');
		closePersona();
		expect(personas.open).toBeNull();
		expect(personas.selectedId).toBeNull();
	});
});

describe('savePersona and removePersona', () => {
	it('creates when there is no id, then reloads the library', async () => {
		const created = await savePersona(null, { name: 'Builder', role: 'Builds' });
		expect(mocks.createPersona).toHaveBeenCalledWith({ name: 'Builder', role: 'Builds' });
		expect(created.id).toBe('id-Builder');
		expect(mocks.listPersonas).toHaveBeenCalledTimes(1);
		expect(personas.open?.id).toBe('id-Builder');
	});

	it('updates by id and takes the saved persona from the daemon', async () => {
		await openPersona('id-Reviewer');
		const saved = await savePersona('id-Reviewer', { role: 'Careful reviewer' });
		expect(mocks.updatePersona).toHaveBeenCalledWith('id-Reviewer', { role: 'Careful reviewer' });
		expect(saved.role).toBe('Careful reviewer');
		expect(personas.open?.role).toBe('Careful reviewer');
	});

	it('deletes, closes the detail if it was open, and reloads', async () => {
		await openPersona('id-Reviewer');
		await removePersona('id-Reviewer');
		expect(mocks.deletePersona).toHaveBeenCalledWith('id-Reviewer');
		expect(personas.open).toBeNull();
		expect(mocks.listPersonas).toHaveBeenCalled();
	});
});

describe('the roster', () => {
	it('loads a project roster in position order and remembers the project', async () => {
		await loadRoster('p-1');
		expect(mocks.getProjectRoster).toHaveBeenCalledWith('p-1');
		expect(personas.roster.map((r) => r.name)).toEqual(['Reviewer', 'Builder']);
		expect(personas.rosterProjectId).toBe('p-1');
	});

	it('sends the whole list on save and keeps the daemon answer', async () => {
		const entries = [
			{ persona_id: 'id-Reviewer', is_default: true, position: 0 },
			{ persona_id: 'id-Builder', is_default: false, position: 1 }
		];
		await saveRoster('p-1', entries);
		expect(mocks.setProjectRoster).toHaveBeenCalledWith('p-1', entries);
		expect(personas.roster.map((r) => r.slug)).toEqual(['reviewer', 'builder']);
		expect(personas.rosterProjectId).toBe('p-1');
	});

	it('reads every project roster for the usage counts, treating a failed read as empty', async () => {
		mocks.getProjectRoster.mockImplementation(async (id: string) => {
			if (id === 'p-1') return [row('Reviewer', 0, true)];
			throw new Error('no roster');
		});
		await loadUsage();
		expect(personas.rosters).toEqual([
			{ project_id: 'p-1', project_name: 'atlas', persona_ids: ['id-Reviewer'] },
			{ project_id: 'p-2', project_name: 'other', persona_ids: [] }
		]);
		expect(projectsUsing('id-Reviewer')).toBe(1);
		expect(projectsUsing('id-Builder')).toBe(0);
	});

	it('names a persona role by slug from the roster, then the library, then falls back to the name', () => {
		personas.roster = [row('Reviewer', 0)];
		personas.items = [persona('Builder', { role: 'Builds things' })];
		expect(personaRole('reviewer', 'Reviewer')).toBe('Reviewer role');
		expect(personaRole('builder', 'Builder')).toBe('Builds things');
		expect(personaRole('ghost', 'Ghost')).toBe('Ghost');
	});
});

describe('followPersonaChanges', () => {
	it('reloads the library once for a burst of persona changes', async () => {
		vi.useFakeTimers();
		const stop = followPersonaChanges();
		const change = { entity: 'persona', action: 'update', id: 'id-Reviewer', key: null, project_id: null, at: 'now' };
		dispatch(change);
		dispatch(change);
		dispatch(change);
		expect(mocks.listPersonas).not.toHaveBeenCalled();
		await vi.advanceTimersByTimeAsync(PERSONA_CHANGE_DEBOUNCE_MS + 5);
		expect(mocks.listPersonas).toHaveBeenCalledTimes(1);
		stop();
	});

	it('reloads the open roster on a project change for that project', async () => {
		vi.useFakeTimers();
		personas.rosterProjectId = 'p-1';
		const stop = followPersonaChanges();
		dispatch({ entity: 'project', action: 'roster', id: 'p-1', key: null, project_id: 'p-1', at: 'now' });
		await vi.advanceTimersByTimeAsync(PERSONA_CHANGE_DEBOUNCE_MS + 5);
		expect(mocks.getProjectRoster).toHaveBeenCalledWith('p-1');
		stop();
	});

	it('stops listening after the returned function runs', async () => {
		vi.useFakeTimers();
		const stop = followPersonaChanges();
		stop();
		dispatch({ entity: 'persona', action: 'create', id: 'x', key: null, project_id: null, at: 'now' });
		await vi.advanceTimersByTimeAsync(PERSONA_CHANGE_DEBOUNCE_MS + 5);
		expect(mocks.listPersonas).not.toHaveBeenCalled();
	});
});
