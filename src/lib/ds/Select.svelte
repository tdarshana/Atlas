<script lang="ts" module>
	export type SelectOption = string | { value: string; label: string };
</script>

<script lang="ts">
	import { untrack } from 'svelte';
	import type { HTMLSelectAttributes } from 'svelte/elements';
	import Icon from './Icon.svelte';
	import IconButton from './IconButton.svelte';

	// The visible control is a button that opens the design system's own menu, so no
	// native popup appears anywhere in the app. A native <select> stays in the DOM,
	// visually hidden, as the value carrier: `bind:value`, a caller's `onchange` reading
	// `currentTarget.value`, the test id and form semantics all keep working against it,
	// and a pick in the menu sets its value and fires `change` on it.
	//
	// `size` is redefined: on a native select it is a row count, here it is the density step.
	interface Props extends Omit<HTMLSelectAttributes, 'class' | 'value' | 'size'> {
		options?: SelectOption[];
		size?: 'md' | 'sm';
		label?: string;
		value?: string;
		class?: string;
		/** When given, a reset arrow follows the label; the caller passes it only while the
		 * value differs from its default, and it puts the default back. */
		onreset?: () => void;
	}

	let {
		options = [],
		size = 'md',
		label,
		id,
		onreset,
		value = $bindable(),
		class: className = '',
		disabled = false,
		...rest
	}: Props = $props();

	const uid = $props.id();
	const fid = $derived(id ?? uid);
	const testid = $derived((rest as Record<string, unknown>)['data-testid'] as string | undefined);

	const cls = $derived(
		['dbm-select', size === 'sm' && 'dbm-select--sm', className].filter(Boolean).join(' ')
	);

	const items = $derived(options.map((o) => (typeof o === 'string' ? { value: o, label: o } : o)));
	const current = $derived(items.find((i) => i.value === value));

	/* The React original is uncontrolled, so a Select with no value shows its first option.
	   Seed the binding once at init to keep that, and to keep the value readable. Later
	   changes to `options` do not re-seed, which is what uncontrolled means. */
	const first = untrack(() => options)[0];
	if (value === undefined && first !== undefined) {
		value = typeof first === 'string' ? first : first.value;
	}

	let open = $state(false);
	let active = $state(0);
	let menuStyle = $state('');
	let root = $state<HTMLElement>();
	let trigger = $state<HTMLButtonElement>();
	let native = $state<HTMLSelectElement>();

	const ROW = 22;

	/** Fixed placement from the trigger's rectangle, so no scrolling ancestor (a board
	 * lane, a dialog body) can clip the menu. Rects are in zoomed pixels under the app's
	 * UI scale while fixed offsets are not, so they are divided by the root's zoom. It
	 * opens upward when the room below is short and the room above is larger. */
	function place(): void {
		if (!trigger) return;
		const z = parseFloat(document.documentElement.style.zoom) || 1;
		const r = trigger.getBoundingClientRect();
		const viewportBottom = window.innerHeight * z;
		const below = viewportBottom - r.bottom;
		const wanted = Math.min(items.length, 8) * ROW + 10;
		const up = below < wanted && r.top > below;
		const anchor = up ? `bottom:${(viewportBottom - r.top) / z + 4}px` : `top:${r.bottom / z + 4}px`;
		menuStyle = `position:fixed;left:${r.left / z}px;width:${r.width / z}px;${anchor}`;
	}

	function show(): void {
		if (disabled || items.length === 0) return;
		active = Math.max(0, items.findIndex((i) => i.value === value));
		place();
		open = true;
	}

	function pick(v: string): void {
		open = false;
		if (v === value) return;
		value = v;
		if (native) {
			native.value = v;
			native.dispatchEvent(new Event('change', { bubbles: true }));
		}
	}

	function move(step: number): void {
		const n = items.length;
		active = (active + step + n) % n;
		document.getElementById(`${fid}-opt-${active}`)?.scrollIntoView?.({ block: 'nearest' });
	}

	function onTriggerKey(e: KeyboardEvent): void {
		if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
			e.preventDefault();
			if (!open) show();
			else move(e.key === 'ArrowDown' ? 1 : -1);
		} else if (e.key === 'Enter' || e.key === ' ') {
			e.preventDefault();
			if (open) pick(items[active].value);
			else show();
		} else if (e.key === 'Escape' && open) {
			e.stopPropagation();
			open = false;
		} else if (e.key === 'Home' && open) {
			e.preventDefault();
			active = 0;
		} else if (e.key === 'End' && open) {
			e.preventDefault();
			active = items.length - 1;
		}
	}

	function onWindowPointerdown(e: PointerEvent): void {
		if (open && root && !root.contains(e.target as Node)) open = false;
	}

	/** The menu is placed once on open, so a scroll or resize underneath it closes it. */
	function onWindowScroll(e: Event): void {
		if (open && root && !root.contains(e.target as Node)) open = false;
	}
</script>

<svelte:window onpointerdown={onWindowPointerdown} onscrollcapture={onWindowScroll} onresize={() => (open = false)} />

{#snippet control()}
	<span class="dbm-select-wrap" bind:this={root}>
		<button
			type="button"
			id={fid}
			class="{cls} trigger"
			aria-haspopup="listbox"
			aria-expanded={open}
			aria-label={label ? undefined : ((rest as Record<string, unknown>)['aria-label'] as string | undefined)}
			{disabled}
			data-testid={testid ? `${testid}-trigger` : undefined}
			bind:this={trigger}
			onclick={() => (open ? (open = false) : show())}
			onkeydown={onTriggerKey}
		>
			<span class="text">{current?.label ?? ''}</span>
		</button>
		<span class="dbm-select-wrap__caret">
			<span
				style="width:0;height:0;border-left:3.5px solid transparent;border-right:3.5px solid transparent;border-top:4px solid var(--text-tertiary)"
			></span>
		</span>
		<!-- The value carrier: hidden from sight and from the tab order, never from callers. -->
		<select
			{...rest}
			class={cls}
			{disabled}
			tabindex="-1"
			aria-hidden="true"
			style="position:absolute;width:0;height:0;margin:0;padding:0;border:0;opacity:0;pointer-events:none"
			bind:this={native}
			bind:value
		>
			{#each items as item (item.value)}
				<option value={item.value}>{item.label}</option>
			{/each}
		</select>
		{#if open}
			<div class="dbm-menu menu" role="listbox" style={menuStyle} data-testid={testid ? `${testid}-menu` : undefined}>
				{#each items as item, i (item.value)}
					<button
						type="button"
						id="{fid}-opt-{i}"
						class="dbm-menu__item item"
						class:active={i === active}
						role="option"
						aria-selected={item.value === value}
						data-testid={testid ? `${testid}-option-${item.value}` : undefined}
						onpointerenter={() => (active = i)}
						onclick={() => pick(item.value)}
					>
						<span class="mark">{#if item.value === value}<Icon name="check" size={12} />{/if}</span>
						<span class="dbm-menu__label">{item.label}</span>
					</button>
				{/each}
			</div>
		{/if}
	</span>
{/snippet}

{#if label}
	<span class="dbm-field">
		<span class="dbm-field__labelrow">
			<label class="dbm-field__label" for={fid}>{label}</label>
			{#if onreset}
				<IconButton size="sm" icon="undo-2" label="Reset {label}" data-testid={testid ? `${testid}-reset` : undefined} onclick={onreset} />
			{/if}
		</span>
		{@render control()}
	</span>
{:else}
	{@render control()}
{/if}

<style>
	/* Element-typed selectors on purpose: the hidden <select> carries a dynamic class, and
	   a bare class selector would make Svelte scope it too, changing its class name. */
	button.trigger {
		display: flex;
		align-items: center;
		text-align: left;
		cursor: pointer;
	}

	button.trigger:disabled {
		cursor: default;
	}

	span.text {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	div.menu {
		z-index: 1000;
		min-width: 0;
		max-height: 240px;
		overflow-y: auto;
	}

	button.item {
		cursor: pointer;
	}

	/* One highlight for the keyboard's row and the pointer's row. */
	button.item.active,
	button.item:hover {
		background: var(--bg-active);
	}

	span.mark {
		display: inline-flex;
		width: 12px;
		justify-content: center;
		flex: none;
	}
</style>
