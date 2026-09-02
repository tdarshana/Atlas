// Projects screen state: the connected list, and the one project a detail page
// shows. Connecting a project needs a directory; inside Tauri that comes from the
// native folder picker, in a browser from a typed path.

import { ApiError } from '$lib/api';
import { api } from '$lib/daemon.svelte';
import type { Project, ProjectContext, Uuid } from '$lib/types';
import { logPath } from './memories.svelte';

export const projects = $state({
	items: [] as Project[],
	loading: false,
	error: null as string | null,
	errorLogPath: null as string | null,
	connecting: false
});

export const projectDetail = $state({
	project: null as Project | null,
	context: null as ProjectContext | null,
	loading: false,
	error: null as string | null,
	errorLogPath: null as string | null,
	refreshing: false
});

/** The Tauri invoke bridge only exists inside the webview. */
export function inTauri(): boolean {
	return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export async function loadProjects(): Promise<void> {
	projects.loading = true;
	try {
		projects.items = await api().listProjects();
		projects.error = null;
		projects.errorLogPath = null;
	} catch (e) {
		projects.items = [];
		projects.error = e instanceof Error ? e.message : String(e);
		projects.errorLogPath = e instanceof ApiError && e.status === 0 ? logPath() : null;
	} finally {
		projects.loading = false;
	}
}

/** Native folder picker; null when it is unavailable or the user cancels. */
export async function pickProjectRoot(): Promise<string | null> {
	if (!inTauri()) return null;
	const { open } = await import('@tauri-apps/plugin-dialog');
	const picked = await open({ directory: true, multiple: false });
	return typeof picked === 'string' ? picked : null;
}

/** Connects `root` and refreshes the list. Throws so the caller can toast. */
export async function connectProject(root: string): Promise<Project> {
	projects.connecting = true;
	try {
		const project = await api().connectProject(root);
		await loadProjects();
		return project;
	} finally {
		projects.connecting = false;
	}
}

export async function loadProject(id: Uuid): Promise<void> {
	projectDetail.loading = true;
	try {
		const project = await api().getProject(id);
		projectDetail.project = project;
		// The context route keys on the root path, not the id.
		projectDetail.context = await api().projectContext(project.root_path);
		projectDetail.error = null;
		projectDetail.errorLogPath = null;
	} catch (e) {
		projectDetail.project = null;
		projectDetail.context = null;
		projectDetail.error = e instanceof Error ? e.message : String(e);
		projectDetail.errorLogPath = e instanceof ApiError && e.status === 0 ? logPath() : null;
	} finally {
		projectDetail.loading = false;
	}
}

/** Rebuilds the profile, then reloads the page's data. Throws so the caller can toast. */
export async function refreshProject(id: Uuid): Promise<void> {
	projectDetail.refreshing = true;
	try {
		await api().refreshProject(id);
		await loadProject(id);
	} finally {
		projectDetail.refreshing = false;
	}
}
