<script lang="ts">
	// The output card (frame 08): a success-tint list-checks header, then one row per
	// thing the run does with what the actions produced. One target handle; nothing
	// leaves the output.
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { Icon } from '$lib/ds';
	import { projects } from '$lib/stores/projects.svelte';
	import { workflow } from '$lib/stores/workflows.svelte';
	import type { NodeKind, OutputNodeData } from '$lib/types';
	import NodeShell from './NodeShell.svelte';

	let { data, selected }: NodeProps & { data: OutputNodeData; type: NodeKind } = $props();

	const boardName = $derived.by(() => {
		const id = workflow.current?.project_id ?? null;
		if (!id) return 'the global';
		return projects.items.find((p) => p.id === id)?.name ?? id;
	});
</script>

<NodeShell width={110} {selected} icon="list-checks" iconColor="var(--success-text)" title="Output">
	{#if data.propose_memories}
		<div class="row">
			<Icon name="braces" size={12} color="var(--text-tertiary)" />
			<span class="clip">Propose memories to Review</span>
		</div>
	{/if}
	{#if data.file_tasks}
		<div class="row">
			<Icon name="columns-3" size={12} color="var(--text-tertiary)" />
			<span class="clip">File tasks on {boardName} board</span>
		</div>
	{/if}
</NodeShell>

<Handle type="target" position={Position.Left} />

<style>
	.row {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 18px;
	}

	.clip {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
</style>
