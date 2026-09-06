// Persona state, shared by the `/personas` route, its side panel, the project Personas
// tab and the board's persona select and chips. The library list is global; the roster
// is the one project's list that was loaded last, because the tab and the board are
// never on screen for two projects at once.

import { api } from '$lib/daemon.svelte';
import { errorMessage } from '$lib/errors';
import type {
	NewPersona,
	Persona,
	PersonaPatch,
	Project,
	RosterEntry,
	RosterRow,
	Uuid
} from '$lib/types';
import { onChange } from './changes.svelte';

export const personas = $state({
	items: [] as Persona[],
	loading: false,
	error: null as string | null,
	/** The tag the side panel lit, narrowing the table; null is every persona. */
	tagFilter: null as string | null,
	/** The row lit in the table; the detail panel shows `open` for it. */
	selectedId: null as Uuid | null,
	open: null as Persona | null,
	openLoading: false,
	openError: null as string | null,
	/** The roster of `rosterProjectId`, in position order. */
	roster: [] as RosterRow[],
	rosterProjectId: null as Uuid | null,
	rosterError: null as string | null,
	/** Every project's roster as persona ids, for the table's `Projects` column and the
	 * side panel's project counts. The daemon has no cross-project route, so this is one
	 * roster read per project. */
	rosters: [] as ProjectRoster[]
});

export interface ProjectRoster {
	project_id: Uuid;
	project_name: string;
	persona_ids: Uuid[];
}

/** Reads every project's roster. A project whose roster cannot be read counts as empty. */
export async function loadUsage(): Promise<void> {
	let projects: Project[];
	try {
		projects = await api().listProjects();
	} catch {
		personas.rosters = [];
		return;
	}
	personas.rosters = await Promise.all(
		projects.map(async (p) => {
			const rows = await api()
				.getProjectRoster(p.id)
				.catch(() => [] as RosterRow[]);
			return { project_id: p.id, project_name: p.name, persona_ids: rows.map((r) => r.persona_id) };
		})
	);
}

/** How many projects have `personaId` on their roster. */
export function projectsUsing(personaId: Uuid): number {
	return personas.rosters.filter((r) => r.persona_ids.includes(personaId)).length;
}

export async function loadPersonas(): Promise<void> {
	personas.loading = true;
	try {
		personas.items = await api().listAgents();
		personas.error = null;
	} catch (e) {
		personas.items = [];
		personas.error = errorMessage(e);
	} finally {
		personas.loading = false;
	}
}

/** Opens one persona in the detail panel by id, slug or name. */
export async function openPersona(idOrSlug: string): Promise<void> {
	personas.openLoading = true;
	personas.openError = null;
	try {
		const p = await api().getAgent(idOrSlug);
		personas.open = p;
		personas.selectedId = p.id;
	} catch (e) {
		personas.open = null;
		personas.openError = errorMessage(e);
	} finally {
		personas.openLoading = false;
	}
}

export function closePersona(): void {
	personas.open = null;
	personas.selectedId = null;
	personas.openError = null;
}

/**
 * Creates when `id` is null, updates otherwise. The daemon answers with the saved
 * persona, so the panel takes its text from the server rather than from the form,
 * then the library reloads so the row matches.
 */
export async function savePersona(id: Uuid | null, patch: PersonaPatch): Promise<Persona> {
	const saved =
		id === null
			? await api().createAgent(patch as NewPersona)
			: await api().updateAgent(id, patch);
	personas.open = saved;
	personas.selectedId = saved.id;
	await loadPersonas();
	return saved;
}

export async function removePersona(id: Uuid): Promise<void> {
	await api().deleteAgent(id);
	if (personas.selectedId === id) closePersona();
	await loadPersonas();
}

export async function loadRoster(projectId: Uuid): Promise<void> {
	personas.rosterProjectId = projectId;
	try {
		personas.roster = await api().getProjectRoster(projectId);
		personas.rosterError = null;
	} catch (e) {
		personas.roster = [];
		personas.rosterError = errorMessage(e);
	}
}

/** Writes the whole roster; the daemon answers with the rows as they now read. */
export async function saveRoster(projectId: Uuid, entries: RosterEntry[]): Promise<void> {
	personas.roster = await api().setProjectRoster(projectId, entries);
	personas.rosterProjectId = projectId;
	personas.rosterError = null;
}

/** A task only carries its persona's name and slug; the role on a chip comes from the
 * roster, then the library, and falls back to the name when neither knows the slug. */
export function personaRole(slug: string, name: string): string {
	const fromRoster = personas.roster.find((r) => r.slug === slug);
	if (fromRoster) return fromRoster.role || name;
	const fromLibrary = personas.items.find((p) => p.slug === slug);
	if (fromLibrary) return fromLibrary.role || name;
	return name;
}

/** How long to wait after a change before reloading, so a burst costs one request. */
export const PERSONA_CHANGE_DEBOUNCE_MS = 150;

let libraryTimer: ReturnType<typeof setTimeout> | null = null;
let rosterTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * Keeps the library and the loaded roster in step with the daemon: a `persona` change
 * reloads the library (and the open persona), a `project` change for the roster's
 * project reloads that roster. Returns the stop function.
 */
export function followPersonaChanges(): () => void {
	const offPersona = onChange('persona', () => {
		if (libraryTimer !== null) clearTimeout(libraryTimer);
		libraryTimer = setTimeout(() => {
			libraryTimer = null;
			void loadPersonas();
			if (personas.selectedId) void openPersona(personas.selectedId);
			if (personas.rosterProjectId) void loadRoster(personas.rosterProjectId);
			if (personas.rosters.length > 0) void loadUsage();
		}, PERSONA_CHANGE_DEBOUNCE_MS);
	});
	const offProject = onChange('project', (change) => {
		const id = personas.rosterProjectId;
		if (!id || (change.id !== id && change.project_id !== id)) return;
		if (rosterTimer !== null) clearTimeout(rosterTimer);
		rosterTimer = setTimeout(() => {
			rosterTimer = null;
			void loadRoster(id);
			if (personas.rosters.length > 0) void loadUsage();
		}, PERSONA_CHANGE_DEBOUNCE_MS);
	});
	return () => {
		offPersona();
		offProject();
		if (libraryTimer !== null) clearTimeout(libraryTimer);
		if (rosterTimer !== null) clearTimeout(rosterTimer);
		libraryTimer = null;
		rosterTimer = null;
	};
}
