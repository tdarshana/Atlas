// The project hub: the tab list behind `/projects/[id]`, the project the layout header
// names, and the header actions the open tab lends it. Loading and refreshing the project
// is still the projects store's job; this one only points at what it holds, so the hub and
// the Projects screen never disagree about the same project.

import type { Snippet } from 'svelte';
import type { Tab } from '$lib/shell';
import type { IconName } from '$lib/ds';
import type { Project, Task } from '$lib/types';
import { deleteProject, loadProject, projectDetail, refreshProject } from './projects.svelte';

/** The route segment the every-project board answers to. It has no project row. */
export const GLOBAL_ID = 'global';

export type ProjectTabId =
	| 'profile'
	| 'board'
	| 'memories'
	| 'practices'
	| 'agents'
	| 'workflows'
	| 'frameworks'
	| 'log'
	| 'settings';

interface TabDef {
	id: ProjectTabId;
	label: string;
	icon: IconName;
	/** Appended to `/projects/<id>`; the Profile tab is the route itself. */
	segment: string;
}

/** Tab order and icons come from the design's `tabs()` helper, frame 02. */
export const PROJECT_TABS: TabDef[] = [
	{ id: 'profile', label: 'Profile', icon: 'info', segment: '' },
	{ id: 'board', label: 'Board', icon: 'columns-3', segment: 'board' },
	{ id: 'memories', label: 'Memories', icon: 'database', segment: 'memories' },
	{ id: 'practices', label: 'Practices', icon: 'book-open', segment: 'practices' },
	{ id: 'agents', label: 'Agents', icon: 'bot', segment: 'agents' },
	{ id: 'workflows', label: 'Workflows', icon: 'git-branch', segment: 'workflows' },
	{ id: 'frameworks', label: 'Frameworks', icon: 'list-checks', segment: 'frameworks' },
	{ id: 'log', label: 'Log', icon: 'history', segment: 'log' },
	{ id: 'settings', label: 'Project settings', icon: 'settings', segment: 'settings' }
];

/** Global is not a project: it has no profile, practices, agents, workflows or settings. */
const GLOBAL_TABS: ProjectTabId[] = ['board', 'memories'];

/** The tabs the header draws for a route id, already carrying their hrefs. */
export function tabsFor(id: string): Tab[] {
	const wanted =
		id === GLOBAL_ID ? PROJECT_TABS.filter((t) => GLOBAL_TABS.includes(t.id)) : PROJECT_TABS;
	return wanted.map((t) => ({
		id: t.id,
		label: t.label,
		icon: t.icon,
		href: t.segment ? `/projects/${id}/${t.segment}` : `/projects/${id}`
	}));
}

/**
 * The open tab, from the pathname alone. `/projects/x` is the Profile tab, and a segment
 * the hub does not have falls back to Profile rather than lighting nothing.
 */
export function tabForPath(path: string): ProjectTabId {
	const parts = path.split('/').filter(Boolean);
	// ['projects', '<id>', '<segment>']
	const segment = parts[0] === 'projects' ? (parts[2] ?? '') : '';
	return PROJECT_TABS.find((t) => t.segment === segment)?.id ?? 'profile';
}

/** The route's project id, or an empty string away from the hub. */
export function idForPath(path: string): string {
	const parts = path.split('/').filter(Boolean);
	return parts[0] === 'projects' ? (parts[1] ?? '') : '';
}

/**
 * The command box reads `Atlas · <project> · <Tab>`. Null off the hub, so the title bar
 * keeps the rail label it had.
 */
export function hubTitle(path: string, project: Project | null): string | null {
	const id = idForPath(path);
	if (!id) return null;
	const name = id === GLOBAL_ID ? 'Global' : (project?.name ?? 'Project');
	const tab = PROJECT_TABS.find((t) => t.id === tabForPath(path));
	return `${name} · ${tab?.label ?? 'Profile'}`;
}

const hub = $state({
	id: '',
	actions: null as Snippet | null
});

/**
 * What the layout header and the open tab both read. `current` points at the projects
 * store rather than copying it, so a refresh there lands here with no second write.
 */
export const project = {
	get id(): string {
		return hub.id;
	},
	get current(): Project | null {
		// Only this route's project. Moving from one project to another leaves the previous
		// one in the store until the new load lands, and naming it would be a lie.
		const held = projectDetail.project;
		return held && held.id === hub.id ? held : null;
	},
	get actions(): Snippet | null {
		return hub.actions;
	},
	get loading(): boolean {
		return projectDetail.loading;
	},
	get error(): string | null {
		return projectDetail.error;
	},
	get errorLogPath(): string | null {
		return projectDetail.errorLogPath;
	},
	get refreshing(): boolean {
		return projectDetail.refreshing;
	},
	get removing(): boolean {
		return projectDetail.removing;
	}
};

/**
 * A tab lends the header its actions for as long as it is mounted. The snippet is defined
 * in the tab and rendered by the layout, so the tab keeps the state its buttons act on.
 */
export function setHeaderActions(actions: Snippet | null): void {
	hub.actions = actions;
}

/**
 * Called by the layout on every route change, and again by its Retry button with `force`.
 * `global` has no project to fetch.
 */
export async function openProject(id: string, force = false): Promise<void> {
	hub.id = id;
	if (!id || id === GLOBAL_ID) return;
	if (!force && projectDetail.project?.id === id) return;
	await loadProject(id);
}

/** Rebuilds the open project's profile. Throws so the caller can toast. */
export async function refresh(): Promise<void> {
	if (!hub.id || hub.id === GLOBAL_ID) return;
	await refreshProject(hub.id);
}

/** Forgets the open project. Throws so the caller can toast. */
export async function remove(): Promise<void> {
	if (!hub.id || hub.id === GLOBAL_ID) return;
	await deleteProject(hub.id);
}

// ---- profile helpers ----

export type TreeKind = 'folder' | 'file';

export interface TreeEntry {
	path: string;
	name: string;
	depth: number;
	kind: TreeKind;
}

interface TreeNode {
	name: string;
	path: string;
	kind: TreeKind;
	children: Map<string, TreeNode>;
}

/**
 * The profile's flat paths as rows to draw. A trailing slash marks a directory, and a
 * path implies every directory above it, so `a/b.rs` alone still draws `a`. Folders come
 * before files within a level, which is the order a file tree is read in.
 */
export function treeRows(paths: string[]): TreeEntry[] {
	const root: TreeNode = { name: '', path: '', kind: 'folder', children: new Map() };
	for (const raw of paths) {
		const isDir = raw.endsWith('/');
		const parts = raw.split('/').filter(Boolean);
		let node = root;
		parts.forEach((part, i) => {
			const leaf = i === parts.length - 1;
			let child = node.children.get(part);
			if (!child) {
				child = {
					name: part,
					path: parts.slice(0, i + 1).join('/'),
					kind: leaf && !isDir ? 'file' : 'folder',
					children: new Map()
				};
				node.children.set(part, child);
			} else if (leaf && !isDir && child.children.size === 0) {
				// The same name arrived first as an implied parent; nothing hangs off it.
				child.kind = 'file';
			}
			node = child;
		});
	}

	const rows: TreeEntry[] = [];
	const walk = (node: TreeNode, depth: number) => {
		const kids = [...node.children.values()].sort((a, b) =>
			a.kind === b.kind ? a.name.localeCompare(b.name) : a.kind === 'folder' ? -1 : 1
		);
		for (const kid of kids) {
			rows.push({ path: kid.path, name: kid.name, depth, kind: kid.kind });
			walk(kid, depth + 1);
		}
	};
	walk(root, 0);
	return rows;
}

/** How recently an actor has to have touched a task to count as active. */
export const ACTIVE_WINDOW_DAYS = 30;

const DAY_MS = 86_400_000;

/**
 * The agents working this project: every distinct actor on a task touched inside the
 * window. Task events would be the exact answer, but reading them costs one request per
 * task, so this reads the actors the task list already carries.
 */
export function activeAgents(tasks: Task[], now: number = Date.now()): string[] {
	const cutoff = now - ACTIVE_WINDOW_DAYS * DAY_MS;
	const seen = new Set<string>();
	for (const task of tasks) {
		const at = new Date(task.updated_at).getTime();
		if (!Number.isFinite(at) || at < cutoff) continue;
		for (const actor of [task.created_by, task.assignee]) {
			const name = actor?.trim();
			if (name) seen.add(name);
		}
	}
	return [...seen].sort();
}

/** `5 · 1 in testing`, the frame's task line. */
export function taskSummary(tasks: Task[], testingStage = 'Testing'): string {
	const testing = tasks.filter((t) => t.stage.toLowerCase() === testingStage.toLowerCase()).length;
	return `${tasks.length} · ${testing} in testing`;
}
