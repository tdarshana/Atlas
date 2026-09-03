// Board screen state: the effective stage list, the tasks that pass the filter bar,
// and the one task open in the drawer. Columns are derived rather than stored, so a
// move only has to change one task's stage and the layout follows.

import { ApiError } from '$lib/api';
import { api } from '$lib/daemon.svelte';
import { errorLogPath, errorMessage } from '$lib/errors';
import type { Stage, Task, TaskDetail, Uuid } from '$lib/types';
import { push } from '$lib/ui/toasts.svelte';

export const SEARCH_DEBOUNCE_MS = 300;

/** The daemon refuses a stage list outside this range, so the editor does too. */
export const MIN_STAGES = 2;
export const MAX_STAGES = 12;

/** Shown when a move is refused because the task changed under us. */
export const CONFLICT_MESSAGE = 'Someone else changed this task; reloaded';

export interface BoardColumn {
	stage: Stage;
	tasks: Task[];
	/** True when this column also holds tasks whose stage is not in the list. */
	strays: boolean;
}

export const board = $state({
	filters: {
		/** Null is every project, which is what the daemon means by no project_id. */
		projectId: null as Uuid | null,
		assignee: '',
		query: '',
		showDone: false
	},
	stages: [] as Stage[],
	/** True when the chosen project overrides the global stage list. */
	overridden: false,
	tasks: [] as Task[],
	loading: false,
	error: null as string | null,
	/** Set only for a connection failure, so the error state can point at the log. */
	errorLogPath: null as string | null,
	/** The key of the task open in the drawer. */
	selected: null as string | null,
	detail: null as TaskDetail | null,
	detailLoading: false,
	detailError: null as string | null
});

/**
 * Every task under its stage, in stage order. A task whose stage is not in the list
 * would otherwise vanish from the board, so it lands in the first column and that
 * column says so; a stage rename that missed a task is the way this happens.
 */
export function deriveColumns(stages: Stage[], tasks: Task[]): BoardColumn[] {
	const columns: BoardColumn[] = stages.map((stage) => ({ stage, tasks: [], strays: false }));
	if (columns.length === 0) return columns;
	const byStage = new Map(columns.map((c) => [c.stage.name, c]));
	for (const task of tasks) {
		const column = byStage.get(task.stage);
		if (column) {
			column.tasks.push(task);
		} else {
			columns[0].tasks.push(task);
			columns[0].strays = true;
		}
	}
	return columns;
}

/** The reason the list is not usable, or null when it is. */
export function validateStages(stages: Stage[]): string | null {
	const names = stages.map((s) => s.name.trim());
	if (names.some((n) => n === '')) return 'Every stage needs a name.';
	if (names.length < MIN_STAGES || names.length > MAX_STAGES) {
		return `A board needs between ${MIN_STAGES} and ${MAX_STAGES} stages.`;
	}
	const seen = new Set<string>();
	for (const name of names) {
		const key = name.toLowerCase();
		if (seen.has(key)) return `Two stages are called "${name}".`;
		seen.add(key);
	}
	if (!stages.some((s) => s.done)) return 'At least one stage has to be a done stage.';
	return null;
}

/** The board's columns for the current stages and tasks. */
export function columns(): BoardColumn[] {
	return deriveColumns(board.stages, board.tasks);
}

/**
 * Bumped by every load. A search debounce coalesces keystrokes but says nothing
 * about requests already in flight, so a slow early load must not land on top of a
 * later one. Mirrors the guard in the memories store.
 */
let generation = 0;

export async function refresh(): Promise<void> {
	const g = ++generation;
	const { projectId, assignee, query, showDone } = board.filters;
	board.loading = true;
	try {
		const client = api();
		const [list, tasks] = await Promise.all([
			client.boardStages(projectId),
			client.listTasks({
				project_id: projectId,
				assignee: assignee.trim() || null,
				query: query.trim() || null,
				include_done: showDone
			})
		]);
		if (g !== generation) return;
		board.stages = list.stages;
		board.overridden = list.overridden;
		board.tasks = tasks;
		board.error = null;
		board.errorLogPath = null;
	} catch (e) {
		if (g !== generation) return;
		board.stages = [];
		board.tasks = [];
		board.error = errorMessage(e);
		board.errorLogPath = errorLogPath(e);
	} finally {
		if (g === generation) board.loading = false;
	}
}

let timer: ReturnType<typeof setTimeout> | null = null;

/** Coalesces keystrokes into one request. */
export function scheduleRefresh(delayMs = SEARCH_DEBOUNCE_MS): void {
	cancelRefresh();
	timer = setTimeout(() => {
		timer = null;
		void refresh();
	}, delayMs);
}

/** Drops a pending debounce so leaving the screen does not fire one more request. */
export function cancelRefresh(): void {
	if (timer !== null) clearTimeout(timer);
	timer = null;
}

/** Its own guard: the drawer can be pointed at a second task while the first is out. */
let detailGeneration = 0;

export async function loadDetail(key: string): Promise<void> {
	const g = ++detailGeneration;
	board.detailLoading = true;
	try {
		const detail = await api().getTask(key);
		if (g !== detailGeneration) return;
		board.detail = detail;
		board.detailError = null;
	} catch (e) {
		if (g !== detailGeneration) return;
		board.detail = null;
		board.detailError = errorMessage(e);
	} finally {
		if (g === detailGeneration) board.detailLoading = false;
	}
}

export function openTask(key: string): void {
	board.selected = key;
	board.detail = null;
	board.detailError = null;
	void loadDetail(key);
}

export function closeTask(): void {
	detailGeneration++;
	board.selected = null;
	board.detail = null;
	board.detailError = null;
	board.detailLoading = false;
}

/** The list and, when it is open, the drawer. Used after a write from the drawer. */
export async function reload(): Promise<void> {
	const key = board.selected;
	await Promise.all([refresh(), key ? loadDetail(key) : Promise.resolve()]);
}

/**
 * Moves a card now and tells the daemon after. The card is in its new column before
 * the request goes out, so the board does not wait on the network; a rejection puts
 * it back. `expected_updated_at` turns someone else's concurrent edit into a 409
 * rather than a silent clobber, and that case reloads instead of just rolling back.
 */
export async function move(key: string, stage: string): Promise<void> {
	const task = board.tasks.find((t) => t.key === key);
	if (!task || task.stage === stage) return;
	const previous = task.stage;
	const expected = task.updated_at;
	task.stage = stage;
	try {
		const moved = await api().moveTask(key, stage, expected);
		const i = board.tasks.findIndex((t) => t.key === key);
		if (i >= 0) board.tasks[i] = moved;
		if (board.selected === key) void loadDetail(key);
	} catch (e) {
		const i = board.tasks.findIndex((t) => t.key === key);
		if (i >= 0) board.tasks[i].stage = previous;
		if (e instanceof ApiError && e.status === 409) {
			push('error', CONFLICT_MESSAGE);
			await refresh();
		} else {
			push('error', errorMessage(e));
		}
	}
}
