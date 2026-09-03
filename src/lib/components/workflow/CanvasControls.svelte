<script lang="ts">
	// Rendered inside `<SvelteFlow>` so `useSvelteFlow`/`useViewport` resolve against its
	// context: the bottom-left zoom stack (frame 08's plus/minus/maximize, 26px cells) and
	// the bottom-right mono `<zoom>% · <n> nodes · <m> edges` reading. No `duration` is
	// passed to any zoom call, which is what keeps pan and zoom instant rather than eased.
	import { Panel, useSvelteFlow, useViewport } from '@xyflow/svelte';
	import { Icon } from '$lib/ds';

	interface Props {
		nodeCount: number;
		edgeCount: number;
	}

	let { nodeCount, edgeCount }: Props = $props();

	const flow = useSvelteFlow();
	const viewport = useViewport();

	const zoomPct = $derived(Math.round(viewport.current.zoom * 100));
</script>

<Panel position="bottom-left">
	<div class="zoom-stack">
		<button type="button" class="cell" aria-label="Zoom in" onclick={() => flow.zoomIn()}>
			<Icon name="plus" size={13} />
		</button>
		<button type="button" class="cell" aria-label="Zoom out" onclick={() => flow.zoomOut()}>
			<Icon name="minus" size={13} />
		</button>
		<button type="button" class="cell" aria-label="Fit view" onclick={() => flow.fitView()}>
			<Icon name="maximize" size={13} />
		</button>
	</div>
</Panel>

<Panel position="bottom-right">
	<span class="mono badge">{zoomPct}% · {nodeCount} nodes · {edgeCount} edges</span>
</Panel>

<style>
	.zoom-stack {
		display: flex;
		flex-direction: column;
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 3px;
		box-shadow: var(--shadow-sm);
	}

	.cell {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 26px;
		height: 26px;
		border: 0;
		border-bottom: 1px solid var(--border-subtle);
		background: transparent;
		color: var(--text-secondary);
		cursor: default;
		transition: var(--transition-hover);
	}

	.cell:last-child {
		border-bottom: 0;
	}

	.cell:hover {
		background: var(--bg-hover);
		color: var(--text-primary);
	}

	.badge {
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
