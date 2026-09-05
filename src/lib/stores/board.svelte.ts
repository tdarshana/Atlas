// Board screen state: the effective stage list, the tasks that pass the filter bar,
// and the one task open in the drawer. Columns are derived rather than stored, so a
// move only has to change one task's stage and the layout follows.

import { ApiError } from '$lib/api';
import { api } from '$lib/daemon.svelte';
import { onChange } from './changes.svelte';
import { errorLogPath, errorMessage } from '$lib/errors';
import { persistSet } from '$lib/platform/persist';
import type {
	FrameworkKind,
	ImportReport,
	ImportWhat,
	Stage,
	Task,
	TaskDetail,
	TaskEvent,
	TaskUpdate,
	Uuid
} from '$lib/types';
import { push } from '$lib/platform/toasts.svelte';

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
	/** How many of `tasks` are those strays, so the header can count only its own. */
	strayCount: number;
}

/** A stage row being edited, and the name it arrived with. */
export interface StageRow {
	name: string;
	/** The name the server has for this row, or null for a row added in the editor. */
	original: string | null;
}

/** Lane geometry from the design: a 340px lane, draggable between these two. */
export const LANE_DEFAULT = 340;
export const LANE_MIN = 220;
export const LANE_MAX = 520;

/** The docked task detail's geometry, the same idea with its own range. */
export const DETAIL_DEFAULT = 340;
export const DETAIL_MIN = 280;
export const DETAIL_MAX = 560;

/** A collapsed lane shows its header and its name on end, so it needs no more than this. */
export const LANE_COLLAPSED = 44;
/** The gap between lanes in the strip; the resize bar sits in its middle. */
export const LANE_GAP = 12;

const clamp = (value: number, min: number, max: number) =>
	Number.isFinite(value) ? Math.min(max, Math.max(min, Math.round(value))) : min;

export const clampLane = (width: number): number => clamp(width, LANE_MIN, LANE_MAX);
export const clampDetail = (width: number): number => clamp(width, DETAIL_MIN, DETAIL_MAX);

/** Lane widths are the person's own arrangement of one board, so they are keyed by it. */
export function laneKey(projectId: Uuid | null): string {
	return `atlas.board.${projectId ?? 'global'}.lanes`;
}

export const DETAIL_KEY = 'atlas.board.detail';
export const DETAIL_MODE_KEY = 'atlas.board.detail.mode';

/** The docked panel sits beside the lanes; the dialog floats over them. */
export type DetailMode = 'docked' | 'modal';

export function loadDetailMode(): DetailMode {
	try {
		if (typeof localStorage === 'undefined') return 'docked';
		return localStorage.getItem(DETAIL_MODE_KEY) === 'modal' ? 'modal' : 'docked';
	} catch {
		return 'docked';
	}
}

export function saveDetailMode(mode: DetailMode): void {
	try {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem(DETAIL_MODE_KEY, mode);
	} catch {
		/* storage is unavailable; the panel docks next time */
	}
	void persistSet(DETAIL_MODE_KEY, mode);
}

/**
 * The widths this board was left at, by stage name. Anything unreadable, of the wrong
 * shape or out of range is dropped rather than thrown: a bad entry must not cost the
 * board every other lane's width.
 */
export function loadLaneWidths(projectId: Uuid | null): Record<string, number> {
	try {
		if (typeof localStorage === 'undefined') return {};
		const raw = localStorage.getItem(laneKey(projectId));
		if (!raw) return {};
		const parsed: unknown = JSON.parse(raw);
		if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
		const out: Record<string, number> = {};
		for (const [stage, width] of Object.entries(parsed as Record<string, unknown>)) {
			if (typeof width === 'number' && Number.isFinite(width)) out[stage] = clampLane(width);
		}
		return out;
	} catch {
		return {};
	}
}

/**
 * The widths for stages the board still has. A rename or a removed column would otherwise
 * leave its entry under the key for good: inert, since only live stage names are looked
 * up, but it accumulates one dead entry per rename.
 */
export function pruneLaneWidths(
	widths: Record<string, number>,
	stages: Stage[]
): Record<string, number> {
	const live = new Set(stages.map((s) => s.name));
	const out: Record<string, number> = {};
	for (const [stage, width] of Object.entries(widths)) {
		if (live.has(stage)) out[stage] = width;
	}
	return out;
}

/**
 * The collapsed stage names still on the board, the same pruning `pruneLaneWidths`
 * does for widths and for the same reason: a rename or a removed column would
 * otherwise leave the name folded for good, and a stage later renamed back to that
 * old name would silently reappear collapsed.
 */
export function pruneCollapsedLanes(collapsed: string[], stages: Stage[]): string[] {
	const live = new Set(stages.map((s) => s.name));
	return collapsed.filter((stage) => live.has(stage));
}

export function saveLaneWidths(projectId: Uuid | null, widths: Record<string, number>): void {
	try {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem(laneKey(projectId), JSON.stringify(widths));
	} catch {
		/* storage is unavailable; the lanes are simply the default width next time */
	}
	void persistSet(laneKey(projectId), widths);
}

const collapsedKey = (projectId: Uuid | null) => `atlas.board.${projectId ?? 'global'}.collapsed`;

/** The stages someone folded with a lane's own button, by name; unreadable storage is empty. */
export function loadCollapsedLanes(projectId: Uuid | null): string[] {
	try {
		if (typeof localStorage === 'undefined') return [];
		const parsed: unknown = JSON.parse(localStorage.getItem(collapsedKey(projectId)) ?? '[]');
		return Array.isArray(parsed) ? parsed.filter((s): s is string => typeof s === 'string') : [];
	} catch {
		return [];
	}
}

export function saveCollapsedLanes(projectId: Uuid | null, stages: string[]): void {
	try {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem(collapsedKey(projectId), JSON.stringify(stages));
	} catch {
		/* storage is unavailable; every lane is open next time */
	}
	void persistSet(collapsedKey(projectId), stages);
}

export function loadDetailWidth(): number {
	try {
		if (typeof localStorage === 'undefined') return DETAIL_DEFAULT;
		const raw = localStorage.getItem(DETAIL_KEY);
		if (raw === null) return DETAIL_DEFAULT;
		const width = Number(raw);
		return Number.isFinite(width) ? clampDetail(width) : DETAIL_DEFAULT;
	} catch {
		return DETAIL_DEFAULT;
	}
}

export function saveDetailWidth(width: number): void {
	const clamped = clampDetail(width);
	try {
		if (typeof localStorage === 'undefined') return;
		localStorage.setItem(DETAIL_KEY, String(clamped));
	} catch {
		/* as above */
	}
	void persistSet(DETAIL_KEY, clamped);
}

/** A lane and whether the column filter has folded it away. */
export interface LaneView {
	column: BoardColumn;
	/** Folded by the column filter. */
	collapsed: boolean;
	/** Folded by the lane's own collapse button. */
	folded: boolean;
}

/**
 * The lanes to draw for a column filter. The filter narrows the board rather than
 * emptying it: the chosen column keeps its cards and the rest fold to a header, so the
 * side panel's click has an effect on the screen without hiding what it left behind. A
 * filter naming no column at all leaves every lane open.
 */
export function visibleLanes(
	columns: BoardColumn[],
	stage: string | null,
	folded: string[] = []
): LaneView[] {
	const chosen = stage && columns.some((c) => c.stage.name === stage) ? stage : null;
	return columns.map((column) => ({
		column,
		collapsed: chosen !== null && column.stage.name !== chosen,
		folded: folded.includes(column.stage.name)
	}));
}

export const board = $state({
	filters: {
		/** Null is every project, which is what the daemon means by no project_id. */
		projectId: null as Uuid | null,
		assignee: '',
		/** A persona slug from the filters panel, applied on screen like `stage`; empty is all. */
		persona: '',
		query: '',
		/** Done tasks show by default; this hides them on request. */
		hideDone: false,
		/** Subtasks show as their own cards by default; this narrows the board to
		 * top-level tasks on request. */
		hideSubtasks: false,
		/**
		 * One column, or null for all of them. Applied on the screen rather than in the
		 * request: the filters panel counts every column from the same list, and a stage
		 * sent to the daemon would empty the counts it is drawn from.
		 */
		stage: null as string | null,
		/** Keep only the tasks nobody has claimed. Local, for the same reason. */
		unassigned: false
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
	/** The keys opened before the current one, newest last, so the detail can step
	 * back from a subtask to the parent it was opened from. Cleared on close. */
	detailHistory: [] as string[],
	detail: null as TaskDetail | null,
	detailLoading: false,
	detailError: null as string | null,
	/** Lane width by stage name for the open board, seeded from `localStorage`. */
	laneWidths: {} as Record<string, number>,
	/** Stage names folded with their own collapse button, for the open board. */
	collapsedLanes: [] as string[],
	/** The docked detail's width, shared by every board. */
	detailWidth: DETAIL_DEFAULT,
	/** Docked beside the lanes or floating as a dialog, shared by every board. */
	detailMode: loadDetailMode() as DetailMode
});

/** Reads the arrangement one board was left in. Called when the project changes. */
export function loadLayout(projectId: Uuid | null): void {
	board.laneWidths = loadLaneWidths(projectId);
	board.collapsedLanes = loadCollapsedLanes(projectId);
	board.detailWidth = loadDetailWidth();
}

/** Folds a lane to its header, or opens it again. Remembered per board. */
export function toggleLaneCollapsed(stage: string): void {
	board.collapsedLanes = board.collapsedLanes.includes(stage)
		? board.collapsedLanes.filter((s) => s !== stage)
		: [...board.collapsedLanes, stage];
	saveCollapsedLanes(board.filters.projectId, board.collapsedLanes);
}

/** The width a lane is drawn at, which is the default until someone drags it. */
export function laneWidth(widths: Record<string, number>, stage: string): number {
	return widths[stage] ?? LANE_DEFAULT;
}

export function setLaneWidth(stage: string, width: number): void {
	board.laneWidths = { ...board.laneWidths, [stage]: clampLane(width) };
	saveLaneWidths(board.filters.projectId, board.laneWidths);
}

export function setDetailWidth(width: number): void {
	board.detailWidth = clampDetail(width);
	saveDetailWidth(board.detailWidth);
}

export function toggleDetailMode(): void {
	board.detailMode = board.detailMode === 'docked' ? 'modal' : 'docked';
	saveDetailMode(board.detailMode);
}

/** The task detail's lower half: which of its three tabs is showing. */
export type DetailTab = 'subtasks' | 'activity' | 'comments';

export const DETAIL_TAB_KEY = 'atlas.board.detail.tab';

function isDetailTab(value: string | null): value is DetailTab {
	return value === 'subtasks' || value === 'activity' || value === 'comments';
}

function loadDetailTab(): DetailTab {
	try {
		if (typeof localStorage === 'undefined') return 'subtasks';
		const raw = localStorage.getItem(DETAIL_TAB_KEY);
		return isDetailTab(raw) ? raw : 'subtasks';
	} catch {
		return 'subtasks';
	}
}

/** Shared across every task, so opening another one keeps the tab that was showing. Svelte
 *  refuses to export a reassigned `$state` binding directly, so this is read through the
 *  getter below rather than as a plain export. */
let currentDetailTab = $state<DetailTab>(loadDetailTab());

export function detailTab(): DetailTab {
	return currentDetailTab;
}

export function setDetailTab(tab: DetailTab): void {
	currentDetailTab = tab;
	try {
		if (typeof localStorage !== 'undefined') localStorage.setItem(DETAIL_TAB_KEY, tab);
	} catch {
		/* storage is unavailable; the tab resets next time */
	}
	void persistSet(DETAIL_TAB_KEY, tab);
}

/**
 * Every task under its stage, in stage order. A task whose stage is not in the list
 * would otherwise vanish from the board, so it lands in the first column and that
 * column says so; a stage rename that missed a task is the way this happens.
 */
export function deriveColumns(stages: Stage[], tasks: Task[]): BoardColumn[] {
	const columns: BoardColumn[] = stages.map((stage) => ({
		stage,
		tasks: [],
		strays: false,
		strayCount: 0
	}));
	if (columns.length === 0) return columns;
	const byStage = new Map(columns.map((c) => [c.stage.name, c]));
	for (const task of tasks) {
		const column = byStage.get(task.stage);
		if (column) {
			column.tasks.push(task);
		} else {
			columns[0].tasks.push(task);
			columns[0].strays = true;
			columns[0].strayCount++;
		}
	}
	return columns;
}

/**
 * Old name to new name for the rows whose name changed. `original` is pinned to the
 * name the server sent, never rewritten mid-session, so renaming A to B and then B to
 * C in one sitting sends `{A: C}` and the tasks standing in A follow all the way.
 */
export function stageRenames(rows: StageRow[]): Record<string, string> {
	const out: Record<string, string> = {};
	for (const row of rows) {
		const name = row.name.trim();
		if (row.original !== null && row.original !== name) out[row.original] = name;
	}
	return out;
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
	const { projectId, assignee, query, hideDone, hideSubtasks } = board.filters;
	const trimmedQuery = query.trim();
	board.loading = true;
	try {
		const client = api();
		const [list, tasks] = await Promise.all([
			client.boardStages(projectId),
			client.listTasks({
				project_id: projectId,
				assignee: assignee.trim() || null,
				query: trimmedQuery || null,
				include_done: !hideDone,
				// No card shows a description, so the list leaves it out (PERF-5).
				brief: true,
				// Subtasks are cards of their own unless hidden, and a search must still
				// find them, so the top-level narrowing applies only to an unsearched board.
				top_level: hideSubtasks && !trimmedQuery ? true : undefined
			})
		]);
		if (g !== generation) return;
		board.stages = list.stages;
		board.overridden = list.overridden;
		board.tasks = tasks;
		// The stage list is only known once it lands, so this is where a width or a
		// collapsed lane belonging to a renamed or removed column is dropped. Guarded
		// on a non-empty list, or a board that failed to answer would take every width
		// and every collapsed lane with it.
		if (list.stages.length > 0) {
			const kept = pruneLaneWidths(board.laneWidths, list.stages);
			if (Object.keys(kept).length !== Object.keys(board.laneWidths).length) {
				board.laneWidths = kept;
				saveLaneWidths(projectId, kept);
			}
			const keptCollapsed = pruneCollapsedLanes(board.collapsedLanes, list.stages);
			if (keptCollapsed.length !== board.collapsedLanes.length) {
				board.collapsedLanes = keptCollapsed;
				saveCollapsedLanes(projectId, keptCollapsed);
			}
		}
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
		absorb(detail);
	} catch (e) {
		if (g !== detailGeneration) return;
		board.detail = null;
		board.detailError = errorMessage(e);
	} finally {
		if (g === detailGeneration) board.detailLoading = false;
	}
}

/**
 * Folds a freshly loaded detail into the list, so the card of the task on screen and
 * the cards of its subtasks show the daemon's stage the moment the panel does. A stage
 * can change from outside the board (an agent claiming a task over MCP, the CLI), and
 * until the next re-list the column would otherwise disagree with the panel.
 */
function absorb(detail: TaskDetail | null): void {
	if (!detail) return;
	for (const fresh of [detail.task, ...detail.children]) {
		const i = board.tasks.findIndex((t) => t.id === fresh.id);
		if (i >= 0) board.tasks[i] = fresh;
	}
}

/** How often the board re-lists itself while it is on screen, as the fallback for the
 * seconds the change stream is down. The stream is what keeps it live. */
export const BOARD_POLL_MS = 30_000;
/** How long the board waits after a change before acting on it, so a burst of writes
 * (an import, an agent moving several tasks) costs one re-list and a lone change
 * costs one GET of that task. */
export const CHANGE_DEBOUNCE_MS = 150;

let pollTimer: ReturnType<typeof setInterval> | null = null;
let changeTimer: ReturnType<typeof setTimeout> | null = null;
/** The keys named by the changes waiting on `changeTimer`, plus whether one arrived
 * without a key, and whether one touched the open task. */
const pendingKeys = new Set<string>();
let pendingKeyless = false;
let pendingTouchesOpen = false;

/**
 * Replaces one row with what the daemon holds for the task, folding its subtasks in
 * the way a loaded detail does. The row is dropped instead when the task is gone or
 * when a hidden-done board no longer shows its stage; any other failure re-lists.
 */
async function patchRow(key: string): Promise<void> {
	try {
		const detail = await api().getTask(key);
		const done = board.stages.some((s) => s.done && s.name === detail.task.stage);
		if (board.filters.hideDone && done) board.tasks = board.tasks.filter((t) => t.key !== key);
		else absorb(detail);
	} catch (e) {
		if (e instanceof ApiError && e.status === 404) board.tasks = board.tasks.filter((t) => t.key !== key);
		else await refresh();
	}
}

/**
 * Keeps the columns in step with the daemon. The change stream is the live path: a
 * moment after a `task` change the board fetches the one task a lone change names and
 * patches its row, or re-lists when a burst names several tasks, a change has no key,
 * the task is not on the board, or a text filter is on. A change that touches the
 * open task or one of its subtasks reloads the detail too, which folds those rows in
 * on its own. A re-list every `intervalMs` while the window is visible and one on
 * each return to the window are the fallback. Columns are the stage, so a task moved
 * by an agent over MCP or by the CLI has to move on screen without a reload. Returns
 * the stop function.
 */
export function startBoardPolling(intervalMs = BOARD_POLL_MS): () => void {
	stopBoardPolling();
	const visible = () => typeof document === 'undefined' || !document.hidden;
	const onFocus = () => {
		if (visible()) void refresh();
	};
	pollTimer = setInterval(() => {
		if (visible()) void refresh();
	}, intervalMs);
	if (typeof window !== 'undefined') {
		window.addEventListener('focus', onFocus);
		document.addEventListener('visibilitychange', onFocus);
	}
	const unsubscribe = onChange('task', (change) => {
		const open = board.detail;
		if (
			open !== null &&
			(change.id === open.task.id || change.id === open.task.parent_id || open.children.some((c) => c.id === change.id))
		) {
			pendingTouchesOpen = true;
		}
		if (change.key) pendingKeys.add(change.key);
		else pendingKeyless = true;
		if (changeTimer !== null) clearTimeout(changeTimer);
		changeTimer = setTimeout(() => {
			changeTimer = null;
			const keys = [...pendingKeys];
			const lone = keys.length === 1 && !pendingKeyless ? keys[0] : null;
			const touchesOpen = pendingTouchesOpen;
			pendingKeys.clear();
			pendingKeyless = false;
			pendingTouchesOpen = false;
			const reloadKey = touchesOpen ? board.selected : null;
			if (reloadKey) void loadDetail(reloadKey);
			if (lone === null || board.filters.query.trim() || board.filters.assignee.trim()) {
				void refresh();
			} else if (reloadKey && (lone === reloadKey || board.detail?.children.some((c) => c.key === lone))) {
				// The detail reload above already folds this row in.
			} else if (board.tasks.some((t) => t.key === lone)) {
				void patchRow(lone);
			} else {
				void refresh();
			}
		}, CHANGE_DEBOUNCE_MS);
	});
	const unsubscribeLag = onChange('lagged', () => void refresh());
	return () => {
		stopBoardPolling();
		unsubscribe();
		unsubscribeLag();
		if (changeTimer !== null) clearTimeout(changeTimer);
		changeTimer = null;
		pendingKeys.clear();
		pendingKeyless = false;
		pendingTouchesOpen = false;
		if (typeof window !== 'undefined') {
			window.removeEventListener('focus', onFocus);
			document.removeEventListener('visibilitychange', onFocus);
		}
	};
}

export function stopBoardPolling(): void {
	if (pollTimer !== null) {
		clearInterval(pollTimer);
		pollTimer = null;
	}
}

/** How many earlier tasks the detail remembers for its back button. */
export const DETAIL_HISTORY_MAX = 20;

export function openTask(key: string): void {
	if (board.selected && board.selected !== key) {
		board.detailHistory = [...board.detailHistory, board.selected].slice(-DETAIL_HISTORY_MAX);
	}
	show(key);
}

/** Reopens the task the current one was opened from, if there is one. */
export function backTask(): void {
	const previous = board.detailHistory.at(-1);
	if (!previous) return;
	board.detailHistory = board.detailHistory.slice(0, -1);
	show(previous);
}

function show(key: string): void {
	board.selected = key;
	board.detail = null;
	board.detailError = null;
	void loadDetail(key);
}

export function closeTask(): void {
	detailGeneration++;
	board.selected = null;
	board.detailHistory = [];
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
	if (task && task.stage === stage) return;
	// The drawer outlives a task's place in `board.tasks`: a search that no longer
	// matches it, or a failed refresh, empties the list while the drawer stays open.
	// The move still has to happen, it just has nothing on screen to move first.
	const previous = task?.stage ?? null;
	const expected = task?.updated_at;
	if (task) task.stage = stage;
	try {
		const moved = await api().moveTask(key, stage, expected);
		const i = board.tasks.findIndex((t) => t.key === key);
		if (i >= 0) board.tasks[i] = moved;
		else await refresh();
		if (board.selected === key) void loadDetail(key);
	} catch (e) {
		// Only undo our own optimism. A refresh that landed mid-flight may have brought
		// a stage someone else set, and that is fresher than the one we started from.
		const current = board.tasks.find((t) => t.key === key);
		if (previous !== null && current && current.stage === stage) current.stage = previous;
		if (e instanceof ApiError && e.status === 409) {
			push('error', CONFLICT_MESSAGE);
			await reload();
		} else {
			push('error', errorMessage(e));
		}
	}
}

// --- Writes from the detail ------------------------------------------------------------
//
// Every write the task detail makes goes through here, so the card in the lane and the
// open detail are patched from the same row the daemon sends back and never disagree
// (ARCH-11). The detail still asks the page to reload afterwards, which brings the
// events and the rows a write touched indirectly (a blocker's `ready`, a child's stage).

/** A write the daemon refused because the task changed under it (HTTP 409). */
export class ConflictError extends Error {
	constructor() {
		super(CONFLICT_MESSAGE);
		this.name = 'ConflictError';
	}
}

/** Replaces the task's row in the list and in the open detail with the daemon's. */
function accept(task: Task): void {
	const i = board.tasks.findIndex((t) => t.id === task.id);
	if (i >= 0) board.tasks[i] = task;
	const detail = board.detail;
	if (!detail) return;
	if (detail.task.id === task.id) detail.task = task;
	else {
		const c = detail.children.findIndex((t) => t.id === task.id);
		if (c >= 0) detail.children[c] = task;
	}
}

/** Patches the task's fields; a stale `expected_updated_at` throws `ConflictError`. */
export async function updateTask(key: string, patch: TaskUpdate): Promise<Task> {
	try {
		const task = await api().updateTask(key, patch);
		accept(task);
		return task;
	} catch (e) {
		if (e instanceof ApiError && e.status === 409) throw new ConflictError();
		throw e;
	}
}

export async function setBlockers(key: string, keys: string[]): Promise<Task> {
	const task = await api().setTaskBlockers(key, keys);
	accept(task);
	return task;
}

export async function claim(key: string): Promise<Task> {
	const task = await api().claimTask(key);
	accept(task);
	return task;
}

/** Appends the new event to the open detail when it is this task's. */
export async function comment(key: string, body: string): Promise<TaskEvent> {
	const event = await api().commentTask(key, body);
	if (board.detail?.task.key === key) board.detail.events.push(event);
	return event;
}

/** Drops the row from the list and from the open detail's children. */
export async function removeTask(key: string): Promise<void> {
	await api().deleteTask(key);
	board.tasks = board.tasks.filter((t) => t.key !== key);
	if (board.detail) board.detail.children = board.detail.children.filter((t) => t.key !== key);
}

/** Re-runs a framework import. Nothing to patch: the report says what changed, and the
 * caller's reload lists it. */
export function importFramework(
	projectId: Uuid,
	framework: FrameworkKind,
	what: ImportWhat
): Promise<ImportReport> {
	return api().importFramework(projectId, framework, what);
}
