// Projects screen state: the connected list, and the one project a detail page
// shows. Connecting a project needs a directory; inside Tauri that comes from the
// native folder picker, in a browser from a typed path.

import { api } from '$lib/daemon.svelte';
import { errorLogPath, errorMessage } from '$lib/errors';
import type { Project, Uuid } from '$lib/types';

export const projects = $state({
	items: [] as Project[],
	loading: false,
	error: null as string | null,
	errorLogPath: null as string | null,
	connecting: false
});

export const projectDetail = $state({
	project: null as Project | null,
	loading: false,
	error: null as string | null,
	errorLogPath: null as string | null,
	refreshing: false,
	removing: false
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
		projects.error = errorMessage(e);
		projects.errorLogPath = errorLogPath(e);
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

/**
 * Bumped by every `loadProject`. Navigating between two project pages, or a refresh
 * landing while the first load is still out, leaves two loads in flight against one
 * piece of state; a load writes only while it is still the newest. Mirrors the guard
 * in the memories store.
 */
let detailGeneration = 0;

export async function loadProject(id: Uuid): Promise<void> {
	const g = ++detailGeneration;
	projectDetail.loading = true;
	try {
		// `getProject` reads the stored row and nothing more. The context route would
		// upsert the project and write an audit row, so visiting a tab would look like
		// an edit; only the header's Refresh rebuilds anything.
		const project = await api().getProject(id);
		if (g !== detailGeneration) return;
		projectDetail.project = project;
		projectDetail.error = null;
		projectDetail.errorLogPath = null;
	} catch (e) {
		if (g !== detailGeneration) return;
		projectDetail.project = null;
		projectDetail.error = errorMessage(e);
		projectDetail.errorLogPath = errorLogPath(e);
	} finally {
		if (g === detailGeneration) projectDetail.loading = false;
	}
}

/**
 * Removes the project and refreshes the list. The detail state is cleared and the
 * generation bumped, so a load still in flight for the deleted project cannot write
 * it back. Throws so the caller can toast.
 */
export async function deleteProject(id: Uuid): Promise<void> {
	projectDetail.removing = true;
	try {
		await api().deleteProject(id);
		detailGeneration++;
		projectDetail.project = null;
		await loadProjects();
	} finally {
		projectDetail.removing = false;
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
