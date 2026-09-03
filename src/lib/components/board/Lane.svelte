<script lang="ts">
	// One stage's lane: a 32px header naming it with a mono count, the cards beneath, and
	// a 6px handle on the right edge that drags the lane wider or narrower.
	//
	// The drag writes straight to the node's own CSS variable rather than to the store,
	// so a pointer move costs one style write and no reactive pass; the store hears about
	// it once, on release, and that is what gets persisted.
	import { Badge, IconButton } from '$lib/ds';
	import type { SelectOption } from '$lib/ds';
	import {
		type BoardColumn,
		clampLane,
		LANE_COLLAPSED,
		LANE_MAX,
		LANE_MIN
	} from '$lib/stores/board.svelte';
	import TaskCard from './TaskCard.svelte';

	interface Props {
		column: BoardColumn;
		width: number;
		/** Folded to a header by the column filter, with the way back out of it. */
		collapsed: boolean;
		stageOptions: SelectOption[];
		selected: string | null;
		onopen: (key: string) => void;
		onmove: (key: string, stage: string) => void;
		onresize: (stage: string, width: number) => void;
		onexpand: () => void;
	}

	let {
		column,
		width,
		collapsed,
		stageOptions,
		selected,
		onopen,
		onmove,
		onresize,
		onexpand
	}: Props = $props();

	const stage = $derived(column.stage.name);
	/** Its own tasks. Strays stand in the first lane and are counted in its title. */
	const count = $derived(column.tasks.length - column.strayCount);

	let node = $state<HTMLElement>();
	let dragging = false;
	let startX = 0;
	let startWidth = 0;
	let live = 0;

	function grab(event: PointerEvent) {
		const handle = event.currentTarget as HTMLElement;
		handle.setPointerCapture(event.pointerId);
		dragging = true;
		startX = event.clientX;
		startWidth = width;
		live = width;
		event.preventDefault();
	}

	function drag(event: PointerEvent) {
		if (!dragging || !node) return;
		live = clampLane(startWidth + (event.clientX - startX));
		node.style.setProperty('--lane-w', `${live}px`);
	}

	function drop(event: PointerEvent) {
		if (!dragging) return;
		dragging = false;
		(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
		onresize(stage, live);
	}

	/**
	 * An interrupted gesture is not a decision. The lane goes back to the width it was
	 * grabbed at rather than persisting wherever the pointer had got to.
	 */
	function cancel(event: PointerEvent) {
		if (!dragging) return;
		dragging = false;
		(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
		node?.style.setProperty('--lane-w', `${startWidth}px`);
	}

	/** The keyboard gets the same range in 20px steps, since a pointer drag has none. */
	function nudge(event: KeyboardEvent) {
		const step = event.key === 'ArrowLeft' ? -20 : event.key === 'ArrowRight' ? 20 : 0;
		if (step === 0) return;
		event.preventDefault();
		onresize(stage, width + step);
	}
</script>

{#if collapsed}
	<section
		class="lane collapsed"
		style="--lane-w:{LANE_COLLAPSED}px"
		data-testid="board-column-{stage}"
		aria-label="{stage}, {count} tasks, folded by the column filter"
	>
		<header>
			<IconButton
				size="sm"
				icon="chevrons-right"
				label="Show all columns"
				data-testid="board-show-all"
				onclick={onexpand}
			/>
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

		<!-- A focusable separator is exactly what a splitter is; the checker reads the role
		     as decorative. -->
		<!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
		<div
			class="handle"
			role="separator"
			aria-orientation="vertical"
			aria-label="Resize {stage}"
			aria-valuenow={width}
			aria-valuemin={LANE_MIN}
			aria-valuemax={LANE_MAX}
			tabindex="0"
			onpointerdown={grab}
			onpointermove={drag}
			onpointerup={drop}
			onpointercancel={cancel}
			onkeydown={nudge}
		></div>
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
	}

	.empty {
		margin: auto;
		padding: 24px 0;
		color: var(--text-tertiary);
		font-size: 12px;
		text-align: center;
	}

	/* Sits over the lane's own right edge, so the pointer finds it without a gap. */
	.handle {
		position: absolute;
		top: 0;
		bottom: 0;
		right: -3px;
		width: 6px;
		cursor: col-resize;
		border-radius: 2px;
		touch-action: none;
	}

	.handle:hover,
	.handle:focus-visible {
		background: var(--border-strong);
		outline: none;
	}
</style>
