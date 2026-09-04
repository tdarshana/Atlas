<script lang="ts">
	// The board's lanes, side by side and scrolling sideways, with the `Add column` slot
	// at the end. Only the strip scrolls: the lanes keep their own height so a long lane
	// scrolls its cards rather than the whole board.
	import { Icon } from '$lib/ds';
	import type { SelectOption } from '$lib/ds';
	import { laneWidth, type LaneView } from '$lib/stores/board.svelte';
	import Lane from './Lane.svelte';

	interface Props {
		lanes: LaneView[];
		/** Lane width by stage name; a stage nobody has dragged takes the default. */
		widths: Record<string, number>;
		stageOptions: SelectOption[];
		selected: string | null;
		onopen: (key: string) => void;
		onmove: (key: string, stage: string) => void;
		onresize: (stage: string, width: number) => void;
		onexpand: () => void;
		ontoggle: (stage: string) => void;
		onaddcolumn: () => void;
	}

	let {
		lanes,
		widths,
		stageOptions,
		selected,
		onopen,
		onmove,
		onresize,
		onexpand,
		ontoggle,
		onaddcolumn
	}: Props = $props();
</script>

<div class="strip" data-testid="board-lanes">
	{#each lanes as lane (lane.column.stage.name)}
		<Lane
			column={lane.column}
			collapsed={lane.collapsed}
			folded={lane.folded}
			width={laneWidth(widths, lane.column.stage.name)}
			{stageOptions}
			{selected}
			{onopen}
			{onmove}
			{onresize}
			{onexpand}
			{ontoggle}
		/>
	{/each}

	<button type="button" class="add" data-testid="board-add-column" onclick={onaddcolumn}>
		<Icon name="plus" size={13} />
		<span>Add column</span>
	</button>
</div>

<style>
	.strip {
		display: flex;
		align-items: flex-start;
		gap: 12px;
		flex: 1;
		min-height: 0;
		overflow-x: auto;
		overflow-y: hidden;
		padding-bottom: 2px;
		/* The docked detail is out of flow and sits over this strip's right edge, so the
		   scroll range has to be lengthened by its width or the last lane and the `Add
		   column` slot end up under it with no scroll left to reach them. The dock sets
		   the variable and keeps it at 0 while no task is open. */
		padding-right: var(--strip-reserve, 0px);
	}

	.add {
		flex: 0 0 160px;
		width: 160px;
		height: 36px;
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 0 10px;
		border: 1px dashed var(--border-default);
		border-radius: 5px;
		background: transparent;
		color: var(--text-tertiary);
		font-family: var(--font-ui);
		font-size: 12px;
		cursor: default;
	}

	.add:hover {
		border-color: var(--border-strong);
		color: var(--text-secondary);
	}
</style>
