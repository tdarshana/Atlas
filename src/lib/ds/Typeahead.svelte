<script lang="ts" module>
	export interface TypeaheadRow {
		value: string;
		label: string;
		hint?: string;
	}
</script>

<script lang="ts" generics="T extends TypeaheadRow">
	// A text box that suggests as you type: the caller owns the rows (already matched to
	// the text) and what a pick means; this draws them in the design system's menu under
	// the box, keeps the first row selected, moves the selection with the arrow keys or
	// the pointer, picks on Enter or click, and closes on Escape, an outside click or a
	// scroll. The menu is fixed-positioned from the box, so no scrolling ancestor clips it.
	import type { Snippet } from 'svelte';
	import type { HTMLInputAttributes } from 'svelte/elements';

	interface Props extends Omit<HTMLInputAttributes, 'class' | 'value'> {
		value?: string;
		/** The suggestions for the current text, in the order to show them (at most a few). */
		rows: T[];
		onpick: (row: T) => void;
		/** Draws one row; the default is the label with the hint dimmed after it. */
		row?: Snippet<[T]>;
		mono?: boolean;
		class?: string;
	}

	let { value = $bindable(''), rows, onpick, row, mono = false, class: className = '', ...rest }: Props = $props();

	const testid = $derived((rest as Record<string, unknown>)['data-testid'] as string | undefined);
	const cls = $derived(['dbm-input', mono && 'dbm-input--mono', className].filter(Boolean).join(' '));

	let focused = $state(false);
	let dismissed = $state(false);
	let active = $state(0);
	let menuStyle = $state('');
	let root = $state<HTMLElement>();
	let input = $state<HTMLInputElement>();

	/** Open while the box has focus, text and rows, until Escape dismisses it for that text. */
	const open = $derived(focused && !dismissed && value.trim() !== '' && rows.length > 0);

	// New rows: the first one is selected and an earlier Escape no longer applies.
	$effect(() => {
		void rows;
		active = 0;
	});

	const ROW = 22;

	function place(): void {
		if (!input) return;
		const z = parseFloat(document.documentElement.style.zoom) || 1;
		const r = input.getBoundingClientRect();
		const viewportBottom = window.innerHeight * z;
		const below = viewportBottom - r.bottom;
		const wanted = Math.min(rows.length, 8) * ROW + 10;
		const up = below < wanted && r.top > below;
		const anchor = up ? `bottom:${(viewportBottom - r.top) / z + 4}px` : `top:${r.bottom / z + 4}px`;
		menuStyle = `position:fixed;left:${r.left / z}px;width:${r.width / z}px;${anchor}`;
	}

	$effect(() => {
		if (open) place();
	});

	function oninput(event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
		value = event.currentTarget.value;
		dismissed = false;
		rest.oninput?.(event);
	}

	function pick(r: T): void {
		dismissed = true;
		onpick(r);
	}

	function onkeydown(event: KeyboardEvent & { currentTarget: EventTarget & HTMLInputElement }) {
		if (open) {
			if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
				event.preventDefault();
				const n = rows.length;
				active = (active + (event.key === 'ArrowDown' ? 1 : -1) + n) % n;
				document.getElementById(`${testid ?? 'ta'}-opt-${active}`)?.scrollIntoView?.({ block: 'nearest' });
				return;
			}
			if (event.key === 'Enter') {
				event.preventDefault();
				pick(rows[active]);
				return;
			}
			if (event.key === 'Escape') {
				event.stopPropagation();
				dismissed = true;
				return;
			}
		}
		rest.onkeydown?.(event);
	}

	function onWindowPointerdown(e: PointerEvent): void {
		if (root && !root.contains(e.target as Node)) focused = false;
	}

	function onWindowScroll(e: Event): void {
		if (open && root && !root.contains(e.target as Node)) dismissed = true;
	}
</script>

<svelte:window onpointerdown={onWindowPointerdown} onscrollcapture={onWindowScroll} onresize={() => (dismissed = true)} />

<span class="wrap" bind:this={root}>
	<input
		{...rest}
		bind:this={input}
		class={cls}
		{value}
		autocomplete="off"
		role="combobox"
		aria-autocomplete="list"
		aria-expanded={open}
		{oninput}
		{onkeydown}
		onfocus={(e) => {
			focused = true;
			rest.onfocus?.(e);
		}}
	/>
	{#if open}
		<div class="dbm-menu menu" role="listbox" style={menuStyle} data-testid={testid ? `${testid}-menu` : undefined}>
			{#each rows as r, i (r.value)}
				<button
					type="button"
					id="{testid ?? 'ta'}-opt-{i}"
					class="dbm-menu__item item"
					class:active={i === active}
					role="option"
					aria-selected={i === active}
					data-testid={testid ? `${testid}-option-${r.value}` : undefined}
					onpointerenter={() => (active = i)}
					onpointerdown={(e) => e.preventDefault()}
					onclick={() => pick(r)}
				>
					{#if row}
						{@render row(r)}
					{:else}
						<span class="dbm-menu__label">{r.label}</span>
						{#if r.hint}<span class="hint">{r.hint}</span>{/if}
					{/if}
				</button>
			{/each}
		</div>
	{/if}
</span>

<style>
	span.wrap {
		display: block;
		width: 100%;
	}

	div.menu {
		z-index: 1000;
		min-width: 0;
		max-height: 240px;
		overflow-y: auto;
	}

	button.item {
		cursor: pointer;
		gap: 8px;
	}

	button.item.active,
	button.item:hover {
		background: var(--bg-active);
	}

	span.hint {
		flex: none;
		color: var(--text-tertiary);
		font-size: var(--text-xs);
	}
</style>
