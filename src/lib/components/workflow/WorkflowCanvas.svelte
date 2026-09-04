<script lang="ts">
	// The Editor tab's canvas (frame 08): the graph store's nodes and edges rendered
	// through the three custom node kinds. Node drag updates positions through the
	// two-way `nodes` binding; edges are one-way and recomputed here so an edge into the
	// output can be dashed and an edge touching the selected node can be accented without
	// storing that transient styling back into the graph the store saves.
	import type { Connection } from '@xyflow/svelte';
	import { SvelteFlow } from '@xyflow/svelte';
	import { connect, workflow } from '$lib/stores/workflows.svelte';
	import { push } from '$lib/ui/toasts.svelte';
	import ActionNode from './ActionNode.svelte';
	import CanvasControls from './CanvasControls.svelte';
	import OutputNode from './OutputNode.svelte';
	import TriggerNode from './TriggerNode.svelte';

	const nodeTypes = { trigger: TriggerNode, action: ActionNode, output: OutputNode };

	const displayEdges = $derived(
		workflow.graph.edges.map((e) => {
			const intoOutput = workflow.graph.nodes.find((n) => n.id === e.target)?.type === 'output';
			const touchesSelected =
				workflow.selectedNodeId != null &&
				(e.source === workflow.selectedNodeId || e.target === workflow.selectedNodeId);
			const style = [
				intoOutput && 'stroke-dasharray:4 3',
				touchesSelected && 'stroke:var(--accent)'
			]
				.filter(Boolean)
				.join(';');
			return { ...e, style: style || undefined };
		})
	);

	function handleConnect(connection: Connection): void {
		const reason = connect(connection.source, connection.target);
		if (reason) push('error', reason);
	}
</script>

<div class="canvas" data-testid="workflow-canvas">
	<SvelteFlow
		bind:nodes={workflow.graph.nodes}
		edges={displayEdges}
		{nodeTypes}
		fitView
		fitViewOptions={{ maxZoom: 1 }}
		minZoom={0.25}
		maxZoom={2}
		proOptions={{ hideAttribution: true }}
		onconnect={handleConnect}
		onnodeclick={({ node }) => (workflow.selectedNodeId = node.id)}
		onpaneclick={() => (workflow.selectedNodeId = null)}
	>
		<CanvasControls
			nodeCount={workflow.graph.nodes.length}
			edgeCount={workflow.graph.edges.length}
		/>
	</SvelteFlow>
</div>

<style>
	.canvas {
		flex: 1;
		position: relative;
		min-height: 0;
		background: var(--bg-base);
		border: 1px solid var(--border-subtle);
		border-radius: 5px;
		overflow: hidden;
	}

	.canvas :global(.svelte-flow) {
		width: 100%;
		height: 100%;
	}
</style>
