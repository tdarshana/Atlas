// Store-level coverage for the workflow editor: graph mutation, connection validation,
// the API <-> canvas graph mapping, save/dirty tracking, the cron hint helper and the
// free-position helper the side panel's "add" actions rely on.

import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Graph, Trigger, Workflow } from '$lib/types';

const mocks = vi.hoisted(() => ({
	listWorkflows: vi.fn(),
	getWorkflow: vi.fn(),
	createWorkflow: vi.fn(),
	patchWorkflow: vi.fn(),
	deleteWorkflow: vi.fn(),
	runWorkflow: vi.fn(),
	listRuns: vi.fn(),
	getRun: vi.fn(),
	cancelRun: vi.fn()
}));

vi.mock('$lib/daemon.svelte', () => ({
	api: () => mocks,
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import {
	ACTION_PRESETS,
	addAction,
	cancelRun,
	connect,
	createWorkflow,
	cronHint,
	filterRuns,
	freePosition,
	fromApiGraph,
	loadRunDetail,
	loadRunHistory,
	NODE_WIDTH,
	openWorkflow,
	removeNode,
	removeWorkflow,
	rerun,
	run,
	save,
	toApiGraph,
	updateNodeData,
	workflow,
	type WFNode
} from './workflows.svelte';
import type { WorkflowRun } from '$lib/types';

const manualTrigger: Trigger = { kind: 'manual', cron: null, prompt: null };

function sampleGraph(): Graph {
	return {
		nodes: [
			{ id: 't', kind: 'trigger', position: { x: 0, y: 0 }, data: manualTrigger },
			{
				id: 'a',
				kind: 'action',
				position: { x: 240, y: 0 },
				data: { name: 'a', instructions: 'do it', agent: 'desktop', practices: [], memories: null }
			},
			{
				id: 'o',
				kind: 'output',
				position: { x: 480, y: 0 },
				data: { propose_memories: true, file_tasks: false }
			}
		],
		edges: [
			{ id: 't->a', source: 't', target: 'a' },
			{ id: 'a->o', source: 'a', target: 'o' }
		]
	};
}

function sampleWorkflow(): Workflow {
	return {
		id: 'wf-1',
		name: 'sample',
		project_id: null,
		description: '',
		trigger: manualTrigger,
		graph: sampleGraph(),
		enabled: true,
		created_at: '2026-09-01T00:00:00Z',
		updated_at: '2026-09-01T00:00:00Z',
		last_run_at: null,
		last_status: null
	};
}

beforeEach(() => {
	for (const fn of Object.values(mocks)) fn.mockReset();
	workflow.list = [];
	workflow.current = null;
	workflow.graph = { nodes: [], edges: [] };
	workflow.dirty = false;
	workflow.selectedNodeId = null;
	workflow.saveError = null;
	workflow.runs = [];
	workflow.history = [];
	workflow.runDetail = null;
	workflow.runDetailError = null;
});

function sampleRun(overrides: Partial<WorkflowRun> = {}): WorkflowRun {
	return {
		id: 'run-1',
		workflow_id: 'wf-1',
		number: 1,
		trigger: 'manual',
		status: 'success',
		started_at: '2026-09-03T09:00:00Z',
		finished_at: '2026-09-03T09:01:00Z',
		summary: null,
		...overrides
	};
}

describe('fromApiGraph / toApiGraph', () => {
	it('round trips a graph through the canvas shape and back', () => {
		const api = sampleGraph();
		const { nodes, edges } = fromApiGraph(api);
		expect(nodes.map((n) => n.type)).toEqual(['trigger', 'action', 'output']);
		expect(nodes[1].data).toEqual(api.nodes[1].data);

		const back = toApiGraph(nodes, edges);
		expect(back).toEqual(api);
	});
});

describe('graph mutation', () => {
	beforeEach(() => {
		workflow.current = sampleWorkflow();
		workflow.graph = fromApiGraph(sampleGraph());
	});

	it('adds a preset action node and marks the graph dirty', () => {
		const before = workflow.graph.nodes.length;
		addAction(ACTION_PRESETS[0]);
		expect(workflow.graph.nodes).toHaveLength(before + 1);
		expect(workflow.graph.nodes.at(-1)?.type).toBe('action');
		expect(workflow.dirty).toBe(true);
	});

	it('removes an action node and every edge touching it', () => {
		removeNode('a');
		expect(workflow.graph.nodes.find((n) => n.id === 'a')).toBeUndefined();
		expect(workflow.graph.edges).toHaveLength(0);
		expect(workflow.dirty).toBe(true);
	});

	it('refuses to remove the trigger or the output', () => {
		removeNode('t');
		removeNode('o');
		expect(workflow.graph.nodes).toHaveLength(3);
		expect(workflow.dirty).toBe(false);
	});

	it('updateNodeData merges a partial into the node and marks dirty', () => {
		updateNodeData('a', { name: 'renamed' });
		const node = workflow.graph.nodes.find((n) => n.id === 'a');
		expect(node?.data).toMatchObject({ name: 'renamed', instructions: 'do it' });
		expect(workflow.dirty).toBe(true);
	});
});

describe('connect validation', () => {
	beforeEach(() => {
		workflow.graph = fromApiGraph(sampleGraph());
	});

	it('refuses a self edge', () => {
		expect(connect('a', 'a')).toMatch(/itself/);
	});

	it('refuses an edge into the trigger', () => {
		expect(connect('a', 't')).toMatch(/trigger/);
	});

	it('refuses an edge out of the output', () => {
		expect(connect('o', 'a')).toMatch(/output/);
	});

	it('refuses a duplicate edge', () => {
		expect(connect('t', 'a')).toMatch(/already exists/);
	});

	it('refuses a connection that would create a cycle', () => {
		// a -> o already exists; o -> a would close a loop through it if it were legal to
		// leave the output, but the output rule already blocks it, so add a second action
		// to prove the cycle check on its own.
		workflow.graph.nodes.push({
			id: 'b',
			type: 'action',
			position: { x: 240, y: 160 },
			data: { name: 'b', instructions: '', agent: 'desktop', practices: [], memories: null }
		});
		expect(connect('a', 'b')).toBeNull();
		expect(connect('b', 'a')).toMatch(/cycle/);
	});

	it('adds the edge and marks the graph dirty once a connection is valid', () => {
		workflow.graph.edges = workflow.graph.edges.filter((e) => e.id !== 't->a');
		expect(workflow.dirty).toBe(false);
		const reason = connect('t', 'a');
		expect(reason).toBeNull();
		expect(workflow.graph.edges.some((e) => e.source === 't' && e.target === 'a')).toBe(true);
		expect(workflow.dirty).toBe(true);
	});
});

describe('cronHint', () => {
	it('reads the documented cron expressions in plain language', () => {
		expect(cronHint('0 9 * * 1-5')).toBe('weekdays at 09:00');
		expect(cronHint('*/15 * * * *')).toBe('every 15 minutes');
		expect(cronHint('0 0 1 * *')).toBe('monthly on day 1 at 00:00');
	});

	it('falls back to a generic reading for anything else', () => {
		expect(cronHint('*/5 * * * *')).toBe('custom schedule');
		expect(cronHint('')).toBe('custom schedule');
	});
});

describe('freePosition', () => {
	it('leaves the first node where it is asked to go', () => {
		const pos = freePosition([], 'action', { x: 280, y: 120 });
		expect(pos).toEqual({ x: 280, y: 120 });
	});

	it('steps a second node clear of a first one at the same spot', () => {
		const first: WFNode = { id: 'a', type: 'action', position: { x: 280, y: 120 }, data: {} as never };
		const pos = freePosition([first], 'action', { x: 280, y: 120 });
		expect(pos.x).toBe(280);
		expect(pos.y).toBeGreaterThan(120);

		// The two cards, at their real widths, must not overlap on either axis.
		const w = NODE_WIDTH.action;
		const overlapsX = pos.x < first.position.x + w && pos.x + w > first.position.x;
		const overlapsY = pos.y < first.position.y + 96 && pos.y + 96 > first.position.y;
		expect(overlapsX && overlapsY).toBe(false);
	});
});

describe('save / dirty tracking', () => {
	it('saves the graph and the trigger node data, then clears dirty', async () => {
		const wf = sampleWorkflow();
		workflow.current = wf;
		workflow.graph = fromApiGraph(sampleGraph());
		updateNodeData('a', { name: 'renamed' });
		expect(workflow.dirty).toBe(true);

		const saved = { ...wf, graph: toApiGraph(workflow.graph.nodes, workflow.graph.edges) };
		mocks.patchWorkflow.mockResolvedValue(saved);

		await save();

		expect(mocks.patchWorkflow).toHaveBeenCalledWith(
			'wf-1',
			expect.objectContaining({ trigger: manualTrigger })
		);
		expect(workflow.dirty).toBe(false);
		expect(workflow.saveError).toBeNull();
	});

	it('keeps the graph dirty and records the message when the daemon rejects it', async () => {
		workflow.current = sampleWorkflow();
		workflow.graph = fromApiGraph(sampleGraph());
		updateNodeData('a', { name: 'renamed' });
		mocks.patchWorkflow.mockRejectedValue(new Error('the graph has a cycle'));

		await expect(save()).rejects.toThrow('the graph has a cycle');
		expect(workflow.dirty).toBe(true);
		expect(workflow.saveError).toBe('the graph has a cycle');
	});
});

describe('openWorkflow / createWorkflow / removeWorkflow', () => {
	it('loads a workflow and resets the local graph to match it', async () => {
		mocks.getWorkflow.mockResolvedValue(sampleWorkflow());
		await openWorkflow('wf-1');
		expect(workflow.current?.id).toBe('wf-1');
		expect(workflow.graph.nodes).toHaveLength(3);
		expect(workflow.dirty).toBe(false);
	});

	it('names a new workflow untitled-<n>, avoiding collisions in the list', async () => {
		workflow.list = [{ ...sampleWorkflow(), name: 'untitled-1' }];
		mocks.createWorkflow.mockImplementation(async (body) => ({ ...sampleWorkflow(), ...body, id: 'wf-2' }));
		const created = await createWorkflow();
		expect(created.name).toBe('untitled-2');
		expect(workflow.list).toHaveLength(2);
	});

	it('removes a workflow and clears it if it was open', async () => {
		workflow.list = [sampleWorkflow()];
		workflow.current = sampleWorkflow();
		mocks.deleteWorkflow.mockResolvedValue(undefined);
		await removeWorkflow('wf-1');
		expect(workflow.list).toHaveLength(0);
		expect(workflow.current).toBeNull();
	});
});

describe('run', () => {
	it('queues a run for the open workflow', async () => {
		workflow.current = sampleWorkflow();
		mocks.runWorkflow.mockResolvedValue({
			id: 'run-1',
			workflow_id: 'wf-1',
			number: 3,
			trigger: 'manual',
			status: 'queued',
			started_at: '2026-09-01T00:00:00Z',
			finished_at: null,
			summary: null
		});
		const r = await run();
		expect(mocks.runWorkflow).toHaveBeenCalledWith('wf-1', undefined, undefined);
		expect(r.number).toBe(3);
	});
});

describe('filterRuns', () => {
	const runs: WorkflowRun[] = [
		sampleRun({ id: 'r1', status: 'success', trigger: 'schedule' }),
		sampleRun({ id: 'r2', status: 'failed', trigger: 'manual' }),
		sampleRun({ id: 'r3', status: 'success', trigger: 'prompt' }),
		sampleRun({ id: 'r4', status: 'cancelled', trigger: 'manual' })
	];

	it('passes every run through for "all"', () => {
		expect(filterRuns(runs, 'all')).toEqual(runs);
	});

	it('keeps only failed runs for "failed"', () => {
		expect(filterRuns(runs, 'failed').map((r) => r.id)).toEqual(['r2']);
	});

	it('keeps only schedule-triggered runs for "schedule"', () => {
		expect(filterRuns(runs, 'schedule').map((r) => r.id)).toEqual(['r1']);
	});

	it('keeps only prompt-triggered runs for "prompt"', () => {
		expect(filterRuns(runs, 'prompt').map((r) => r.id)).toEqual(['r3']);
	});

	it('keeps only manual-triggered runs for "manual"', () => {
		expect(filterRuns(runs, 'manual').map((r) => r.id)).toEqual(['r2', 'r4']);
	});
});

describe('loadRunHistory / loadRunDetail', () => {
	it('loads the open workflow\'s run list into history, newest first as the daemon answers', async () => {
		workflow.current = sampleWorkflow();
		const runs = [sampleRun({ id: 'r2' }), sampleRun({ id: 'r1' })];
		mocks.listRuns.mockResolvedValue(runs);
		await loadRunHistory();
		expect(mocks.listRuns).toHaveBeenCalledWith('wf-1', 200);
		expect(workflow.history).toEqual(runs);
	});

	it('loads one run\'s detail', async () => {
		const detail = { run: sampleRun(), steps: [] };
		mocks.getRun.mockResolvedValue(detail);
		await loadRunDetail('run-1');
		expect(workflow.runDetail).toEqual(detail);
		expect(workflow.runDetailError).toBeNull();
	});

	it('drops a stale response that resolves after a newer selection\'s response already landed', async () => {
		// Run A is selected first (its request is slow, e.g. still running), then the user
		// quickly selects run B (whose request is fast). A's response arriving late must
		// not clobber B's, even though A's request started first.
		const detailA = { run: sampleRun({ id: 'run-a', status: 'running', finished_at: null }), steps: [] };
		const detailB = { run: sampleRun({ id: 'run-b', status: 'success' }), steps: [] };
		let resolveA!: (v: typeof detailA) => void;
		const pendingA = new Promise<typeof detailA>((resolve) => {
			resolveA = resolve;
		});
		mocks.getRun.mockImplementationOnce(() => pendingA);
		mocks.getRun.mockImplementationOnce(async () => detailB);

		const callA = loadRunDetail('run-a');
		const callB = loadRunDetail('run-b');
		await callB;
		expect(workflow.runDetail?.run.id).toBe('run-b');

		resolveA(detailA);
		await callA;
		// A's late response is dropped: B's selection is still what is shown.
		expect(workflow.runDetail?.run.id).toBe('run-b');
	});
});

describe('cancelRun', () => {
	it('cancels a run and updates it everywhere it is held', async () => {
		const queued = sampleRun({ status: 'queued', finished_at: null });
		const cancelled = { ...queued, status: 'cancelled' as const };
		workflow.history = [queued];
		workflow.runDetail = { run: queued, steps: [] };
		mocks.cancelRun.mockResolvedValue(cancelled);

		const updated = await cancelRun('run-1');

		expect(mocks.cancelRun).toHaveBeenCalledWith('run-1');
		expect(updated.status).toBe('cancelled');
		expect(workflow.history[0].status).toBe('cancelled');
		expect(workflow.runDetail?.run.status).toBe('cancelled');
	});
});

describe('rerun', () => {
	it('calls the API with the same trigger the given run used', async () => {
		workflow.current = sampleWorkflow();
		const original = sampleRun({ trigger: 'schedule' });
		const created = sampleRun({ id: 'run-2', number: 2, status: 'queued', finished_at: null });
		mocks.runWorkflow.mockResolvedValue(created);

		const result = await rerun(original);

		expect(mocks.runWorkflow).toHaveBeenCalledWith('wf-1', 'schedule');
		expect(result).toEqual(created);
		expect(workflow.history[0]).toEqual(created);
	});

	it('throws when no workflow is open', async () => {
		workflow.current = null;
		await expect(rerun(sampleRun())).rejects.toThrow('No workflow is open');
	});
});

describe('runError', () => {
	it('reads the reason the runner writes into a failed run summary, and nothing otherwise', async () => {
		const { runError } = await import('./workflows.svelte');
		expect(runError({ summary: { error: 'extraction is disabled' } })).toBe('extraction is disabled');
		expect(runError({ summary: { error: '' } })).toBeNull();
		expect(runError({ summary: null })).toBeNull();
		expect(runError(null)).toBeNull();
	});
});
