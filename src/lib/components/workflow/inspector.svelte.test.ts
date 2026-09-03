// @vitest-environment jsdom
// The Inspector edits the selected node's data straight into the graph store; this pins
// that typing into a field marks the graph dirty without a Save, which is the whole
// "edits stay local until Save" rule from the caller's point of view.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';
import type { Workflow } from '$lib/types';

vi.mock('$lib/daemon.svelte', () => ({
	api: () => ({
		listAgents: async () => [],
		listDocs: async () => [],
		listProjects: async () => []
	}),
	daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
	baseUrl: () => 'http://127.0.0.1:7433',
	boot: async () => {}
}));

import Inspector from './Inspector.svelte';
import { fromApiGraph, workflow } from '$lib/stores/workflows.svelte';

afterEach(cleanup);

function sampleWorkflow(): Workflow {
	return {
		id: 'wf-1',
		name: 'sample',
		project_id: null,
		description: '',
		trigger: { kind: 'manual', cron: null, prompt: null },
		graph: {
			nodes: [
				{ id: 't', kind: 'trigger', position: { x: 0, y: 0 }, data: { kind: 'manual', cron: null, prompt: null } },
				{
					id: 'a',
					kind: 'action',
					position: { x: 240, y: 0 },
					data: { name: 'a', instructions: '', agent: 'desktop', practices: [], memories: null }
				},
				{
					id: 'o',
					kind: 'output',
					position: { x: 480, y: 0 },
					data: { propose_memories: false, file_tasks: false }
				}
			],
			edges: [
				{ id: 't->a', source: 't', target: 'a' },
				{ id: 'a->o', source: 'a', target: 'o' }
			]
		},
		enabled: true,
		created_at: '2026-09-01T00:00:00Z',
		updated_at: '2026-09-01T00:00:00Z',
		last_run_at: null,
		last_status: null
	};
}

describe('Inspector dirty tracking', () => {
	it('renders nothing when no node is selected', () => {
		workflow.current = sampleWorkflow();
		workflow.graph = fromApiGraph(sampleWorkflow().graph);
		workflow.selectedNodeId = null;
		workflow.dirty = false;

		const { queryByTestId } = render(Inspector);
		expect(queryByTestId('inspector')).toBeNull();
	});

	it('marks the graph dirty as soon as a field is edited, before any Save', async () => {
		workflow.current = sampleWorkflow();
		workflow.graph = fromApiGraph(sampleWorkflow().graph);
		workflow.selectedNodeId = 'a';
		workflow.dirty = false;

		const { getByTestId } = render(Inspector);
		expect(workflow.dirty).toBe(false);

		const name = getByTestId('inspector-name') as HTMLInputElement;
		await fireEvent.input(name, { target: { value: 'renamed action' } });

		expect(workflow.dirty).toBe(true);
		const node = workflow.graph.nodes.find((n) => n.id === 'a');
		expect(node?.data).toMatchObject({ name: 'renamed action' });
	});

	it('shows the node counter and switches form fields with the selected node', () => {
		workflow.current = sampleWorkflow();
		workflow.graph = fromApiGraph(sampleWorkflow().graph);
		workflow.selectedNodeId = 't';
		workflow.dirty = false;

		const { getByTestId } = render(Inspector);
		expect(getByTestId('inspector').textContent).toContain('node 1 of 3');
		expect(getByTestId('inspector-trigger-kind')).toBeTruthy();
	});
});
