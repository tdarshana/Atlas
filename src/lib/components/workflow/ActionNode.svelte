<script lang="ts">
	// The action card (frame 08): a play header naming the step, then the agent as a
	// badge, the attached practices and the memory source it reads, each on its own row.
	// One target handle and one source handle: an action sits in the middle of the run.
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { Badge, Icon } from '$lib/ds';
	import { projects } from '$lib/stores/projects.svelte';
	import type { ActionNodeData, NodeKind } from '$lib/types';
	import NodeShell from './NodeShell.svelte';

	let { data, selected }: NodeProps & { data: ActionNodeData; type: NodeKind } = $props();

	const practiceLine = $derived(
		data.practices.length === 0
			? 'No practices'
			: `${data.practices.length} ${data.practices.length === 1 ? 'practice' : 'practices'} · ${data.practices.join(', ')}`
	);

	const memoryLine = $derived.by(() => {
		const m = data.memories;
		if (!m) return 'Memories: off';
		const scope = m.project_id ? (projects.items.find((p) => p.id === m.project_id)?.name ?? m.project_id) : 'global';
		const kinds = m.kinds.length ? m.kinds.join(', ') : 'any kind';
		return `Memories: ${kinds} · ${scope}`;
	});
</script>

<NodeShell width={200} {selected} icon="play" title={data.name}>
	<div class="row">
		<Badge tone="accent" mono>{data.agent}</Badge>
	</div>
	<div class="row">
		<Icon name="book-open" size={12} color="var(--text-tertiary)" />
		<span class="clip">{practiceLine}</span>
	</div>
	<div class="row">
		<Icon name="database" size={12} color="var(--text-tertiary)" />
		<span class="clip">{memoryLine}</span>
	</div>
</NodeShell>

<Handle type="target" position={Position.Left} />
<Handle type="source" position={Position.Right} />

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
