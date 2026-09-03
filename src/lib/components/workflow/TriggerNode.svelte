<script lang="ts">
	// The trigger card (frame 08): a warning-tint zap header with the trigger kind as a
	// badge, then whichever of the cron or the prompt line applies. One source handle;
	// nothing ever connects into a trigger.
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { Badge, Icon } from '$lib/ds';
	import type { NodeKind, Trigger } from '$lib/types';
	import NodeShell from './NodeShell.svelte';

	let { data, selected }: NodeProps & { data: Trigger; type: NodeKind } = $props();
</script>

<NodeShell width={170} {selected} icon="zap" iconColor="var(--warning-text)" title="Trigger">
	{#snippet headerRight()}
		<Badge tone="warning">{data.kind}</Badge>
	{/snippet}

	{#if data.kind === 'schedule'}
		<div class="row">
			<Icon name="clock" size={12} color="var(--text-tertiary)" />
			<span class="clip mono">{data.cron || 'no schedule set'}</span>
		</div>
	{:else if data.kind === 'prompt'}
		<div class="row">
			<Icon name="terminal" size={12} color="var(--text-tertiary)" />
			<span class="clip">Prompt: "{data.prompt || ''}"</span>
		</div>
	{:else}
		<div class="row">
			<Icon name="terminal" size={12} color="var(--text-tertiary)" />
			<span class="clip">Manual only</span>
		</div>
	{/if}
</NodeShell>

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
