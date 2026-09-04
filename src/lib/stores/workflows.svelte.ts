// The workflow editor's state: the workflow list, the open workflow, and a local editable
// copy of its graph in `@xyflow/svelte`'s node/edge shape. Canvas edits (add, remove,
// connect, drag, field edits) only ever touch `graph`; nothing reaches the daemon until
// `save()`, which is what "graph edits stay local until Save" means in practice.

import type { Edge as FlowEdge, Node as FlowNode } from '@xyflow/svelte';
import { api } from '$lib/daemon.svelte';
import { errorMessage } from '$lib/errors';
import type {
	ActionNodeData,
	Graph,
	GraphPosition,
	NodeData,
	NodeKind,
	NewWorkflow,
	OutputNodeData,
	RunDetail,
	RunStatus,
	Trigger,
	TriggerKind,
	Workflow,
	WorkflowRun
} from '$lib/types';

// `@xyflow/svelte`'s `Node<T>` requires `T extends Record<string, unknown>`, which none of
// the three plain `NodeData` shapes satisfy on their own; the intersection gives each an
// index signature for that check without touching the wire types themselves.
export type WFNode = FlowNode<NodeData & Record<string, unknown>, NodeKind>;
export type WFEdge = FlowEdge;

// ---- node data type guards ----

export function isTriggerData(data: NodeData): data is Trigger {
	return 'kind' in data;
}

export function isActionData(data: NodeData): data is ActionNodeData {
	return 'name' in data;
}

export function isOutputData(data: NodeData): data is OutputNodeData {
	return 'propose_memories' in data;
}

// ---- graph <-> API mapping ----

/** The local canvas shape back to the wire `Graph`. */
export function toApiGraph(nodes: WFNode[], edges: WFEdge[]): Graph {
	return {
		nodes: nodes.map((n) => ({
			id: n.id,
			kind: (n.type ?? 'action') as NodeKind,
			position: { x: n.position.x, y: n.position.y },
			data: n.data
		})),
		edges: edges.map((e) => ({ id: e.id, source: e.source, target: e.target }))
	};
}

/** The wire `Graph` into the canvas shape; `type` is what `nodeTypes` matches on. */
export function fromApiGraph(graph: Graph): { nodes: WFNode[]; edges: WFEdge[] } {
	return {
		nodes: graph.nodes.map((n) => ({
			id: n.id,
			type: n.kind,
			position: { x: n.position.x, y: n.position.y },
			data: n.data as NodeData & Record<string, unknown>
		})),
		edges: graph.edges.map((e) => ({ id: e.id, source: e.source, target: e.target }))
	};
}

// ---- layout ----

/** Card width per kind (frame 08); used only to keep freshly added nodes from overlapping. */
export const NODE_WIDTH: Record<NodeKind, number> = { trigger: 170, action: 200, output: 110 };
/** A generous stand-in for card height: header plus up to three 18px rows plus padding. */
const NODE_HEIGHT = 96;
const GAP = 32;

/** Where a brand new workflow's three starter nodes sit. */
export const DEFAULT_POSITIONS: Record<NodeKind, GraphPosition> = {
	trigger: { x: 40, y: 120 },
	action: { x: 280, y: 120 },
	output: { x: 560, y: 120 }
};

/**
 * The first position at or below `start` that does not overlap an existing node's card,
 * stepping down a card height at a time. Used for both the "New workflow" starter graph
 * and every node the side panel adds afterward, so two additions never land on top of
 * each other.
 */
export function freePosition(
	nodes: WFNode[],
	kind: NodeKind,
	start: GraphPosition = { x: 280, y: 120 }
): GraphPosition {
	const w = NODE_WIDTH[kind];
	const overlaps = (x: number, y: number) =>
		nodes.some((n) => {
			const nw = NODE_WIDTH[n.type as NodeKind] ?? NODE_WIDTH.action;
			return (
				x < n.position.x + nw + GAP &&
				x + w + GAP > n.position.x &&
				y < n.position.y + NODE_HEIGHT + GAP &&
				y + NODE_HEIGHT + GAP > n.position.y
			);
		});
	let { x, y } = start;
	let tries = 0;
	while (overlaps(x, y) && tries < 200) {
		y += NODE_HEIGHT + GAP;
		tries++;
	}
	return { x, y };
}

// ---- connection validation ----

/** Adding `source -> target` closes a cycle exactly when `target` can already reach
 * `source` through the edges that already exist. */
function createsCycle(edges: WFEdge[], source: string, target: string): boolean {
	const out = new Map<string, string[]>();
	for (const e of edges) {
		if (!out.has(e.source)) out.set(e.source, []);
		out.get(e.source)!.push(e.target);
	}
	const seen = new Set<string>();
	const stack = [target];
	while (stack.length) {
		const id = stack.pop()!;
		if (id === source) return true;
		if (seen.has(id)) continue;
		seen.add(id);
		for (const next of out.get(id) ?? []) stack.push(next);
	}
	return false;
}

/**
 * The client-side half of what `graph::validate` enforces on the daemon: no self edge,
 * nothing into the trigger, nothing out of the output, no duplicate edge, no cycle.
 * Returns the reason a connection is refused, or null when it is fine to add.
 */
export function validateConnection(
	nodes: WFNode[],
	edges: WFEdge[],
	source: string,
	target: string
): string | null {
	if (source === target) return 'A node cannot connect to itself';
	const sourceNode = nodes.find((n) => n.id === source);
	const targetNode = nodes.find((n) => n.id === target);
	if (!sourceNode || !targetNode) return 'Unknown node';
	if (targetNode.type === 'trigger') return 'Nothing connects into the trigger';
	if (sourceNode.type === 'output') return 'Nothing connects out of the output';
	if (edges.some((e) => e.source === source && e.target === target)) {
		return 'That connection already exists';
	}
	if (createsCycle(edges, source, target)) return 'That connection would create a cycle';
	return null;
}

// ---- cron hint ----

/** A plain-language reading of a handful of common cron expressions; anything else just
 * says it is one, since parsing every cron shape is the daemon's job, not the editor's. */
export function cronHint(expr: string): string {
	switch (expr.trim()) {
		case '0 9 * * 1-5':
			return 'weekdays at 09:00';
		case '*/15 * * * *':
			return 'every 15 minutes';
		case '0 0 1 * *':
			return 'monthly on day 1 at 00:00';
		default:
			return 'custom schedule';
	}
}

// ---- action presets (the side panel's ACTIONS LIBRARY) ----

export interface ActionPreset {
	id: string;
	label: string;
	data: ActionNodeData;
}

export const ACTION_PRESETS: ActionPreset[] = [
	{
		id: 'ingest-transcripts',
		label: 'Ingest transcripts',
		data: {
			name: 'Ingest transcripts',
			instructions: 'Ingest the session transcripts from the previous step and extract candidate memories.',
			agent: 'cli/codex',
			practices: [],
			memories: null
		}
	},
	{
		id: 'collect-open-tasks',
		label: 'Collect open tasks',
		data: {
			name: 'Collect open tasks',
			instructions: 'List the open tasks on the board that need attention.',
			agent: 'desktop',
			practices: [],
			memories: { kinds: ['todo'], tags: [], limit: 20, project_id: null }
		}
	},
	{
		id: 'summarise-decisions',
		label: 'Summarise decisions',
		data: {
			name: 'Summarise decisions',
			instructions: 'Summarise the decisions made and propose each as a memory.',
			agent: 'cli/claude-code',
			practices: [],
			memories: { kinds: ['decision', 'insight'], tags: ['atlas'], limit: 20, project_id: null }
		}
	},
	{
		id: 'blank-action',
		label: 'Blank action',
		data: { name: 'New action', instructions: '', agent: 'desktop', practices: [], memories: null }
	}
];

// ---- the store ----

export const workflow = $state({
	list: [] as Workflow[],
	loading: false,
	error: null as string | null,
	loaded: false,

	current: null as Workflow | null,
	graph: { nodes: [] as WFNode[], edges: [] as WFEdge[] },
	dirty: false,
	selectedNodeId: null as string | null,
	saving: false,
	/** The daemon's validation message from the last failed `save()`. */
	saveError: null as string | null,

	runs: [] as WorkflowRun[],
	runsLoading: false,

	/** The Run history tab's own list, separate from `runs` above (the side panel's last
	 * few) so loading the full history there never trims what the side panel shows. */
	history: [] as WorkflowRun[],
	historyLoading: false,

	runDetail: null as RunDetail | null,
	runDetailLoading: false,
	runDetailError: null as string | null
});

export async function loadWorkflows(projectId?: string | null): Promise<void> {
	workflow.loading = true;
	try {
		workflow.list = await api().listWorkflows(projectId);
		workflow.error = null;
		workflow.loaded = true;
	} catch (e) {
		workflow.error = errorMessage(e);
	} finally {
		workflow.loading = false;
	}
}

/** Loads a workflow and resets the local graph to match it. Always refetches, so a Save
 * elsewhere and a return to this workflow never leaves a stale graph on screen. */
export async function openWorkflow(id: string): Promise<void> {
	workflow.loading = true;
	try {
		const w = await api().getWorkflow(id);
		workflow.current = w;
		workflow.graph = fromApiGraph(w.graph);
		workflow.dirty = false;
		workflow.selectedNodeId = null;
		workflow.saveError = null;
		workflow.error = null;
	} catch (e) {
		workflow.error = errorMessage(e);
	} finally {
		workflow.loading = false;
	}
}

function untitledName(): string {
	const used = new Set(workflow.list.map((w) => w.name));
	let n = 1;
	while (used.has(`untitled-${n}`)) n++;
	return `untitled-${n}`;
}

/** Creates `untitled-<n>` with a manual trigger, one action node `main`, and an output
 * node, then pushes it onto the list. The caller navigates to it. `projectId` binds the
 * new workflow to that project (the project Workflows tab's own "New workflow"); left
 * out, it is global, matching the side panel's own "New workflow…" row. */
export async function createWorkflow(projectId?: string | null): Promise<Workflow> {
	const graph: Graph = {
		nodes: [
			{
				id: 'trigger',
				kind: 'trigger',
				position: DEFAULT_POSITIONS.trigger,
				data: { kind: 'manual', cron: null, prompt: null }
			},
			{
				id: 'main',
				kind: 'action',
				position: DEFAULT_POSITIONS.action,
				data: { name: 'main', instructions: '', agent: 'desktop', practices: [], memories: null }
			},
			{
				id: 'output',
				kind: 'output',
				position: DEFAULT_POSITIONS.output,
				data: { propose_memories: false, file_tasks: false }
			}
		],
		edges: [
			{ id: 'trigger->main', source: 'trigger', target: 'main' },
			{ id: 'main->output', source: 'main', target: 'output' }
		]
	};
	const body: NewWorkflow = {
		name: untitledName(),
		project_id: projectId ?? undefined,
		trigger: { kind: 'manual', cron: null, prompt: null },
		graph
	};
	const created = await api().createWorkflow(body);
	workflow.list.push(created);
	return created;
}

export async function removeWorkflow(id: string): Promise<void> {
	await api().deleteWorkflow(id);
	const i = workflow.list.findIndex((w) => w.id === id);
	if (i >= 0) workflow.list.splice(i, 1);
	if (workflow.current?.id === id) {
		workflow.current = null;
		workflow.graph = { nodes: [], edges: [] };
		workflow.dirty = false;
	}
}

/** Adds a preset action node at the first free spot near the canvas centre. */
export function addAction(preset: ActionPreset): void {
	const pos = freePosition(workflow.graph.nodes, 'action');
	const id = `${preset.id}-${crypto.randomUUID().slice(0, 8)}`;
	workflow.graph.nodes.push({ id, type: 'action', position: pos, data: { ...preset.data } });
	workflow.dirty = true;
}

/** Removes an action node and any edge touching it. The trigger and the output are the
 * graph's fixed endpoints, so removing either is refused rather than silently ignored. */
export function removeNode(id: string): void {
	const node = workflow.graph.nodes.find((n) => n.id === id);
	if (!node || node.type !== 'action') return;
	workflow.graph.nodes = workflow.graph.nodes.filter((n) => n.id !== id);
	workflow.graph.edges = workflow.graph.edges.filter((e) => e.source !== id && e.target !== id);
	if (workflow.selectedNodeId === id) workflow.selectedNodeId = null;
	workflow.dirty = true;
}

/** Validates and, if the connection is allowed, adds the edge. Returns the refusal
 * reason so the caller can toast it, or null once the edge is added. */
export function connect(source: string, target: string): string | null {
	const reason = validateConnection(workflow.graph.nodes, workflow.graph.edges, source, target);
	if (reason) return reason;
	workflow.graph.edges.push({ id: `${source}->${target}`, source, target });
	workflow.dirty = true;
	return null;
}

/** Merges a partial update into a node's `data`. The caller is responsible for shaping
 * the partial to the node's own kind; the store does not re-check it. */
export function updateNodeData(id: string, patch: Partial<NodeData>): void {
	const node = workflow.graph.nodes.find((n) => n.id === id);
	if (!node) return;
	node.data = { ...node.data, ...patch } as NodeData & Record<string, unknown>;
	workflow.dirty = true;
}

/** PATCHes the graph and the trigger (read from the trigger node's own data) to the
 * daemon. Throws the daemon's validation message on failure, which the Inspector shows
 * under its header via `workflow.saveError`. */
export async function save(): Promise<Workflow> {
	const current = workflow.current;
	if (!current) throw new Error('No workflow is open');
	const triggerNode = workflow.graph.nodes.find((n) => n.type === 'trigger');
	if (!triggerNode || !isTriggerData(triggerNode.data)) {
		throw new Error('The graph has no trigger node');
	}
	workflow.saving = true;
	workflow.saveError = null;
	try {
		const graph = toApiGraph(workflow.graph.nodes, workflow.graph.edges);
		const saved = await api().patchWorkflow(current.id, { graph, trigger: triggerNode.data });
		workflow.current = saved;
		workflow.graph = fromApiGraph(saved.graph);
		workflow.dirty = false;
		const i = workflow.list.findIndex((w) => w.id === saved.id);
		if (i >= 0) workflow.list[i] = saved;
		return saved;
	} catch (e) {
		workflow.saveError = errorMessage(e);
		throw e;
	} finally {
		workflow.saving = false;
	}
}

/** Queues a run for the open workflow. Throws so the caller can toast it. Once queued,
 * the header badge, the side panel's RUNS group and the history list are refreshed
 * every 2 s until the run ends (bounded), so a run started from the editor is seen
 * through without a manual reload. */
export async function run(trigger?: 'manual' | 'schedule' | 'prompt', input?: unknown): Promise<WorkflowRun> {
	const current = workflow.current;
	if (!current) throw new Error('No workflow is open');
	const created = await api().runWorkflow(current.id, trigger, input);
	void followRun(current.id, created.id);
	return created;
}

const FOLLOW_INTERVAL_MS = 2000;
const FOLLOW_MAX_TICKS = 150;
const TERMINAL: ReadonlySet<string> = new Set(['success', 'failed', 'cancelled']);

/** Reloads the open workflow, its recent runs and the history list until `runId`
 * reaches a terminal status or the bound is hit; stops early if another workflow
 * is opened in the meantime. */
async function followRun(workflowId: string, runId: string): Promise<void> {
	for (let tick = 0; tick < FOLLOW_MAX_TICKS; tick++) {
		await new Promise((r) => setTimeout(r, FOLLOW_INTERVAL_MS));
		if (workflow.current?.id !== workflowId) return;
		try {
			const [fresh, runs] = await Promise.all([api().getWorkflow(workflowId), api().listRuns(workflowId, 5)]);
			if (workflow.current?.id !== workflowId) return;
			workflow.current = { ...workflow.current, last_run_at: fresh.last_run_at, last_status: fresh.last_status };
			const i = workflow.list.findIndex((w) => w.id === workflowId);
			if (i >= 0) workflow.list[i] = { ...workflow.list[i], last_run_at: fresh.last_run_at, last_status: fresh.last_status };
			workflow.runs = runs;
			if (workflow.history.length > 0 || workflow.historyLoading) void loadRunHistory();
			const status = runs.find((r) => r.id === runId)?.status;
			if (status && TERMINAL.has(status)) return;
		} catch {
			// The daemon may be busy or restarting; the next tick tries again.
		}
	}
}

/** The last few runs of the open workflow, newest first, for the side panel and the
 * Run history tab's own loader to seed from. */
export async function loadRuns(limit = 5): Promise<void> {
	const current = workflow.current;
	if (!current) return;
	workflow.runsLoading = true;
	try {
		workflow.runs = await api().listRuns(current.id, limit);
	} catch {
		workflow.runs = [];
	} finally {
		workflow.runsLoading = false;
	}
}

/** `<name> · <n> nodes · <m> edges` and an ` · unsaved` suffix while `dirty`, per the
 * status bar rule in the brief. */
export function statusLine(): string {
	const current = workflow.current;
	if (!current) return '';
	const nodes = workflow.graph.nodes.length;
	const edges = workflow.graph.edges.length;
	const base = `${current.name} · ${nodes} nodes · ${edges} edges`;
	return workflow.dirty ? `${base} · unsaved` : base;
}

// ---- run history ----

/** Badge tone and label for a run's or a run's `trigger` field, shared by the runs
 * table, the run detail header and the project tab's Recent runs card. */
export const RUN_STATUS_TONE: Record<RunStatus, 'success' | 'danger' | 'warning' | 'neutral'> = {
	success: 'success',
	failed: 'danger',
	cancelled: 'neutral',
	queued: 'warning',
	running: 'warning'
};

export const RUN_STATUS_LABEL: Record<RunStatus, string> = {
	success: 'ok',
	failed: 'failed',
	cancelled: 'cancelled',
	queued: 'queued',
	running: 'running'
};

export const TRIGGER_TONE: Record<TriggerKind, 'warning' | 'accent' | 'neutral'> = {
	schedule: 'warning',
	prompt: 'accent',
	manual: 'neutral'
};

export type RunFilterValue = 'all' | 'failed' | 'schedule' | 'prompt' | 'manual';

/** The Run history card's filter Select, in the order frame 08.1 lists them. */
export const RUN_FILTER_OPTIONS: { value: RunFilterValue; label: string }[] = [
	{ value: 'all', label: 'All runs' },
	{ value: 'failed', label: 'Failed only' },
	{ value: 'schedule', label: 'Scheduled' },
	{ value: 'prompt', label: 'Prompted' },
	{ value: 'manual', label: 'Manual' }
];

/** Narrows a run list to one filter value; `'all'` (and anything else) passes every
 * run through unchanged. */
export function filterRuns(runs: WorkflowRun[], filter: RunFilterValue): WorkflowRun[] {
	switch (filter) {
		case 'failed':
			return runs.filter((r) => r.status === 'failed');
		case 'schedule':
			return runs.filter((r) => r.trigger === 'schedule');
		case 'prompt':
			return runs.filter((r) => r.trigger === 'prompt');
		case 'manual':
			return runs.filter((r) => r.trigger === 'manual');
		default:
			return runs;
	}
}

/** The open workflow's full run list, newest first, for the Run history tab. */
export async function loadRunHistory(limit = 200): Promise<void> {
	const current = workflow.current;
	if (!current) return;
	workflow.historyLoading = true;
	try {
		workflow.history = await api().listRuns(current.id, limit);
	} catch {
		workflow.history = [];
	} finally {
		workflow.historyLoading = false;
	}
}

/** Bumped by every `loadRunDetail` call and captured per request, so a response that
 * outlives a newer one (the poll's own tick racing a fresh selection) never lands: only
 * the reply belonging to the highest generation issued so far is allowed to write
 * `workflow.runDetail`. Selecting run A, then quickly run B, then A's slow `GET
 * /runs/A` resolving after B's fast one, is exactly the race this exists for. */
let runDetailGeneration = 0;

/** One run's full detail (the run plus every step and its log), for the run detail
 * card. Errors are kept on `runDetailError` rather than thrown, since a poll tick
 * failing should not blow up the caller. A response for a request superseded by a
 * later call to this function (a different run selected, or the next poll tick already
 * under way) is dropped rather than applied. */
export async function loadRunDetail(runId: string): Promise<void> {
	const generation = ++runDetailGeneration;
	workflow.runDetailLoading = true;
	try {
		const detail = await api().getRun(runId);
		if (generation !== runDetailGeneration) return;
		workflow.runDetail = detail;
		workflow.runDetailError = null;
	} catch (e) {
		if (generation !== runDetailGeneration) return;
		workflow.runDetailError = errorMessage(e);
	} finally {
		if (generation === runDetailGeneration) workflow.runDetailLoading = false;
	}
}

/** Applies a run update (from a cancel or a poll) everywhere the run appears, so the
 * side panel, the history table and the open detail card never disagree. */
function applyRunUpdate(updated: WorkflowRun): void {
	const hi = workflow.history.findIndex((r) => r.id === updated.id);
	if (hi >= 0) workflow.history[hi] = updated;
	const ri = workflow.runs.findIndex((r) => r.id === updated.id);
	if (ri >= 0) workflow.runs[ri] = updated;
	if (workflow.runDetail?.run.id === updated.id) workflow.runDetail.run = updated;
}

/** Cancels a queued or running run. Throws so the caller can toast it; a run already
 * in a terminal status answers `Conflict` (409), per `WorkflowRepo::cancel_run`. */
export async function cancelRun(runId: string): Promise<WorkflowRun> {
	const updated = await api().cancelRun(runId);
	applyRunUpdate(updated);
	return updated;
}

/** Starts a new run of the open workflow with the same trigger kind an earlier run
 * used, then adds it to the top of the history list so the caller can select it. */
export async function rerun(run: WorkflowRun): Promise<WorkflowRun> {
	const current = workflow.current;
	if (!current) throw new Error('No workflow is open');
	const created = await api().runWorkflow(current.id, run.trigger);
	workflow.history = [created, ...workflow.history];
	return created;
}
