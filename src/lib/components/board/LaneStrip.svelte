<script lang="ts">
	// The board's lanes, side by side and scrolling sideways, with the `Add column` slot
	// at the end. Only the strip scrolls: the lanes keep their own height so a long lane
	// scrolls its cards rather than the whole board.
	import { Icon } from '$lib/ds';
	import type { SelectOption } from '$lib/ds';
	import { laneWidth, place, type LaneView } from '$lib/stores/board.svelte';
	import type { Task } from '$lib/types';
	import Lane from './Lane.svelte';
	import { clearDrag, drag, DRAG_THRESHOLD_PX, indexForY, isSameSpot, markDropped, targetPosition } from './dnd.svelte';

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

	/** A press that may become a drag: the card, its element and where the pointer started. */
	let pending: { task: Task; el: HTMLElement; x: number; y: number } | null = null;

	function zoom(): number {
		return parseFloat(document.documentElement.style.zoom) || 1;
	}

	function onCardPress(event: PointerEvent, task: Task): void {
		const el = (event.currentTarget as HTMLElement | null) ?? (event.target as HTMLElement).closest<HTMLElement>('[data-card-key]');
		if (!el) return;
		pending = { task, el, x: event.clientX, y: event.clientY };
	}

	/** The lane and insertion index under the pointer, from the cards' rectangles. */
	function locate(x: number, y: number): { stage: string; index: number } | null {
		const el = document.elementFromPoint(x, y) as HTMLElement | null;
		const body = el?.closest<HTMLElement>('[data-lane]') ?? el?.closest<HTMLElement>('.lane')?.querySelector<HTMLElement>('[data-lane]') ?? null;
		if (!body) return null;
		const stage = body.dataset.lane ?? '';
		const mids = [...body.querySelectorAll<HTMLElement>('[data-card-key]')]
			.filter((c) => c.dataset.cardKey !== drag.key)
			.map((c) => {
				const r = c.getBoundingClientRect();
				return r.top + r.height / 2;
			});
		return { stage, index: indexForY(mids, y) };
	}

	function onPointerMove(event: PointerEvent): void {
		if (pending && !drag.key) {
			if (Math.hypot(event.clientX - pending.x, event.clientY - pending.y) < DRAG_THRESHOLD_PX) return;
			// The ghost is the card: its markup as it stands, at its own size, held where
			// the pointer pressed it. Rects are zoomed pixels; the ghost is fixed in unzoomed.
			const z = zoom();
			const r = pending.el.getBoundingClientRect();
			drag.html = pending.el.outerHTML;
			drag.width = r.width / z;
			drag.height = r.height / z;
			drag.grabX = (pending.x - r.left) / z;
			drag.grabY = (pending.y - r.top) / z;
			drag.key = pending.task.key;
			drag.title = pending.task.title;
			drag.fromStage = pending.task.stage;
		}
		if (!drag.key) return;
		event.preventDefault();
		drag.x = event.clientX / zoom();
		drag.y = event.clientY / zoom();
		const at = locate(event.clientX, event.clientY);
		drag.overStage = at?.stage ?? null;
		drag.overIndex = at?.index ?? -1;
	}

	function onPointerUp(): void {
		const key = drag.key;
		if (key && drag.overStage !== null) {
			const stage = drag.overStage;
			const column = lanes.find((l) => l.column.stage.name === stage)?.column;
			const tasks = column?.tasks ?? [];
			if (!isSameSpot(tasks, stage, drag.overIndex, key)) {
				void place(key, stage, targetPosition(tasks, drag.overIndex, key));
			}
			markDropped();
		}
		pending = null;
		if (key) clearDrag();
	}

	function onKeydown(event: KeyboardEvent): void {
		if (event.key === 'Escape' && drag.key) {
			pending = null;
			clearDrag();
		}
	}
</script>

<svelte:window onpointermove={onPointerMove} onpointerup={onPointerUp} onpointercancel={onPointerUp} onkeydown={onKeydown} />

{#if drag.key}
	<!-- The card's own markup, inert: nothing inside it can be clicked mid-drag. -->
	<div
		class="ghost"
		style="left:{drag.x - drag.grabX}px;top:{drag.y - drag.grabY}px;width:{drag.width}px;height:{drag.height}px"
		data-testid="drag-ghost"
		aria-hidden="true"
	>
		{@html drag.html}
	</div>
{/if}

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
			ondragstart={onCardPress}
		/>
	{/each}

	<button type="button" class="add" data-testid="board-add-column" onclick={onaddcolumn}>
		<Icon name="plus" size={13} />
		<span>Add column</span>
	</button>
</div>

<style>
	.strip {
		user-select: none;
		display: flex;
		align-items: flex-start;
		gap: 12px;
		flex: 1;
		min-height: 0;
		overflow-x: auto;
		overflow-y: hidden;
		/* The strip reaches down through the panel's 16px padding so its scrollbar sits
		   on the panel's bottom edge, while the lanes keep 12px of clearance above it. */
		margin-bottom: -16px;
		padding-bottom: 12px;
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
	.ghost {
		position: fixed;
		z-index: 2000;
		pointer-events: none;
		box-shadow: var(--shadow-lg);
		border-radius: 5px;
	}

	/* The cloned card fills the ghost exactly. */
	.ghost :global(> [data-card-key]) {
		width: 100%;
		height: 100%;
		margin: 0;
	}
</style>
