<script lang="ts">
	// One stage's lane: a 32px header naming it with a mono count, the cards beneath, and
	// a 6px handle on the right edge that drags the lane wider or narrower.
	//
	// The drag writes straight to the node's own CSS variable rather than to the store,
	// so a pointer move costs one style write and no reactive pass; the store hears about
	// it once, on release, and that is what gets persisted.
	import { Badge, IconButton } from '$lib/ds';
	import type { SelectOption } from '$lib/ds';
	import ResizeBar from '$lib/ui/ResizeBar.svelte';
	import {
		type BoardColumn,
		LANE_COLLAPSED,
		LANE_GAP,
		LANE_MAX,
		LANE_MIN
	} from '$lib/stores/board.svelte';
	import TaskCard from './TaskCard.svelte';

	interface Props {
		column: BoardColumn;
		width: number;
		/** Folded to a header by the column filter, with the way back out of it. */
		collapsed: boolean;
		/** Folded by its own header button; `ontoggle` opens it again. */
		folded: boolean;
		stageOptions: SelectOption[];
		selected: string | null;
		onopen: (key: string) => void;
		onmove: (key: string, stage: string) => void;
		onresize: (stage: string, width: number) => void;
		onexpand: () => void;
		ontoggle: (stage: string) => void;
	}

	let {
		column,
		width,
		collapsed,
		folded,
		stageOptions,
		selected,
		onopen,
		onmove,
		onresize,
		onexpand,
		ontoggle
	}: Props = $props();

	const stage = $derived(column.stage.name);
	/** Its own tasks. Strays stand in the first lane and are counted in its title. */
	const count = $derived(column.tasks.length - column.strayCount);

	let node = $state<HTMLElement>();

	/** The drag paints the node's own CSS variable: one style write per move, no
	    reactive pass. The store hears the final width on release and persists it. */
	function paint(live: number) {
		node?.style.setProperty('--lane-w', `${live}px`);
	}
</script>

{#if collapsed || folded}
	<section
		class="lane collapsed"
		style="--lane-w:{LANE_COLLAPSED}px"
		data-testid="board-column-{stage}"
		aria-label="{stage}, {count} tasks, {folded ? 'collapsed' : 'folded by the column filter'}"
	>
		<header>
			{#if folded}
				<IconButton
					size="sm"
					icon="chevrons-right"
					label="Expand {stage}"
					data-testid="board-expand-{stage}"
					onclick={() => ontoggle(stage)}
				/>
			{:else}
				<IconButton
					size="sm"
					icon="chevrons-right"
					label="Show all columns"
					data-testid="board-show-all"
					onclick={onexpand}
				/>
			{/if}
		</header>
		<span class="tally">{count}</span>
		<!-- On end rather than hidden: a folded lane still has to say which one it is. -->
		<h2 class="sideways">{stage}</h2>
	</section>
{:else}
	<section
		bind:this={node}
		class="lane"
		style="--lane-w:{width}px"
		data-testid="board-column-{stage}"
	>
		<header>
			<h2>{stage}</h2>
			<Badge
				mono
				data-testid="board-count-{stage}"
				title={column.strays
					? `plus ${column.strayCount} from a stage the board no longer has`
					: undefined}
			>
				{count}
			</Badge>
			<span class="spacer"></span>
			<IconButton
				size="sm"
				icon="chevrons-left"
				label="Collapse {stage}"
				data-testid="board-collapse-{stage}"
				onclick={() => ontoggle(stage)}
			/>
		</header>

		<div class="body">
			{#each column.tasks as task (task.id)}
				<TaskCard
					{task}
					{stageOptions}
					selected={selected === task.key}
					{onopen}
					{onmove}
				/>
			{:else}
				<p class="empty">Nothing here</p>
			{/each}
		</div>

		<ResizeBar
			label="Resize {stage}"
			value={width}
			min={LANE_MIN}
			max={LANE_MAX}
			gap={LANE_GAP}
			onlive={paint}
			onresize={(w) => onresize(stage, w)}
			testid="board-resize-{stage}"
		/>
	</section>
{/if}

<style>
	.lane {
		position: relative;
		flex: 0 0 var(--lane-w);
		width: var(--lane-w);
		align-self: stretch;
		display: flex;
		flex-direction: column;
		min-height: 0;
		max-height: 100%;
		padding: 8px;
		/* The lane is the recessed surface and the card the raised one, per frame 03. */
		background: var(--bg-base);
		border: 1px solid var(--border-subtle);
		border-radius: 5px;
	}

	.lane.collapsed {
		align-items: center;
		gap: 6px;
		overflow: hidden;
	}

	.sideways {
		writing-mode: vertical-rl;
		min-height: 0;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	header {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 2px;
	}

	h2 {
		margin: 0;
		font-size: 11px;
		font-weight: 600;
		letter-spacing: 0.04em;
		text-transform: uppercase;
		color: var(--text-tertiary);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.tally {
		font-family: var(--font-mono);
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.body {
		display: flex;
		flex-direction: column;
		gap: 8px;
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		/* The list reaches across the lane's 8px right padding and pads itself back by
		   the same 8px, so a lane with no scrollbar keeps the cards 8px from the lane
		   edge, and a lane with the 10px bar shows the bar in the padding, its thumb
		   flush against that 8px gap: the same gap as the left side and between cards. */
		margin-right: -8px;
		padding-right: 8px;
	}

	/* The shared thumb is inset 2px all round; here the inset moves to the outer side
	   so the thumb starts exactly 8px from the cards and ends 4px from the lane edge. */
	.body::-webkit-scrollbar-thumb {
		border-width: 2px 4px 2px 0;
	}

	.empty {
		margin: auto;
		padding: 24px 0;
		color: var(--text-tertiary);
		font-size: 12px;
		text-align: center;
	}

	.spacer {
		flex: 1;
	}
</style>
