<script lang="ts">
	// A splitter that lives in the gap between two panels: a 10px hit zone centred on the
	// gap, with a 2px accent line that shows after a short hover, while dragging or when
	// focused. The host is `position: relative` and owns the width; this reports live
	// widths during a drag so the host can paint them without a reactive pass, then the
	// final width on release. An interrupted gesture reports the width it started from.
	interface Props {
		label: string;
		value: number;
		min: number;
		max: number;
		/** The gap between the two panels, so the bar sits in its middle. */
		gap?: number;
		onlive?: (width: number) => void;
		onresize: (width: number) => void;
		testid?: string;
	}

	let { label, value, min, max, gap = 6, onlive, onresize, testid }: Props = $props();

	let dragging = $state(false);
	let startX = 0;
	let startWidth = 0;
	let live = 0;

	const clampWidth = (w: number) => Math.min(max, Math.max(min, w));

	function grab(event: PointerEvent) {
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
		dragging = true;
		startX = event.clientX;
		startWidth = value;
		live = value;
		event.preventDefault();
	}

	function drag(event: PointerEvent) {
		if (!dragging) return;
		live = clampWidth(startWidth + (event.clientX - startX));
		onlive?.(live);
	}

	function drop(event: PointerEvent) {
		if (!dragging) return;
		dragging = false;
		(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
		onresize(live);
	}

	function cancel(event: PointerEvent) {
		if (!dragging) return;
		dragging = false;
		(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
		onlive?.(startWidth);
	}

	/** The keyboard gets the same range in 20px steps, since a pointer drag has none. */
	function nudge(event: KeyboardEvent) {
		const step = event.key === 'ArrowLeft' ? -20 : event.key === 'ArrowRight' ? 20 : 0;
		if (step === 0) return;
		event.preventDefault();
		onresize(clampWidth(value + step));
	}
</script>

<!-- A focusable separator is exactly what a splitter is; the checker reads the role as
     decorative. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_no_noninteractive_tabindex -->
<div
	class="bar"
	class:dragging
	role="separator"
	aria-orientation="vertical"
	aria-label={label}
	aria-valuenow={value}
	aria-valuemin={min}
	aria-valuemax={max}
	tabindex="0"
	style="--rb-gap:{gap}px"
	data-testid={testid}
	onpointerdown={grab}
	onpointermove={drag}
	onpointerup={drop}
	onpointercancel={cancel}
	onkeydown={nudge}
>
	<span class="line"></span>
</div>

<style>
	.bar {
		position: absolute;
		top: 0;
		bottom: 0;
		right: calc(var(--rb-gap) / -2 - 5px);
		width: 10px;
		display: flex;
		justify-content: center;
		cursor: col-resize;
		touch-action: none;
		z-index: 1;
	}

	.bar:focus-visible {
		outline: none;
	}

	.line {
		width: 2px;
		height: 100%;
		border-radius: 1px;
		background: var(--accent);
		opacity: 0;
		/* A pause before it shows, so a pointer crossing the gap does not flash it. */
		transition: opacity 120ms ease 160ms;
	}

	.bar:hover .line,
	.bar:focus-visible .line,
	.bar.dragging .line {
		opacity: 1;
	}

	.bar.dragging .line {
		transition-delay: 0ms;
	}
</style>
