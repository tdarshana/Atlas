<script lang="ts">
	// A multi-select dropdown in the Select's clothes: the trigger reads like a select and
	// lists what is chosen; the popover is a `dbm-menu` with a filter box and one checkbox
	// per option. There is no DBMan original for this, so it borrows the select's control
	// and the menu's surface.
	import type { SelectOption } from './Select.svelte';
	import Checkbox from './Checkbox.svelte';
	import Icon from './Icon.svelte';

	interface Props {
		options?: SelectOption[];
		/** The chosen values, in the order they were picked. */
		value?: string[];
		label?: string;
		placeholder?: string;
		size?: 'md' | 'sm';
		id?: string;
		disabled?: boolean;
		/** Shown in the list when nothing matches, or there is nothing to pick. */
		emptyText?: string;
		onchange?: (value: string[]) => void;
		/** The trigger's test id; the filter box gets `-search` and each option `-<value>`. */
		testId?: string;
		class?: string;
	}

	let {
		options = [],
		value = $bindable([]),
		label,
		placeholder = 'Select…',
		size = 'md',
		id,
		disabled = false,
		emptyText = 'Nothing to pick.',
		onchange,
		testId,
		class: className = ''
	}: Props = $props();

	const uid = $props.id();
	const fid = $derived(id ?? uid);

	let open = $state(false);
	let query = $state('');
	let above = $state(false);
	let root = $state<HTMLElement>();
	let trigger = $state<HTMLButtonElement>();
	/** Inline position of the menu, relative to the trigger's offset parent. */
	let place = $state('');

	const items = $derived(options.map((o) => (typeof o === 'string' ? { value: o, label: o } : o)));

	/** A stored value the options no longer name is kept, labelled by itself, so nothing
	 * silently drops off the record. */
	const rows = $derived.by(() => {
		const known = new Set(items.map((o) => o.value));
		const orphans = value.filter((v) => !known.has(v)).map((v) => ({ value: v, label: v }));
		const q = query.trim().toLowerCase();
		return [...orphans, ...items].filter(
			(o) => q === '' || o.label.toLowerCase().includes(q) || o.value.toLowerCase().includes(q)
		);
	});

	const labelOf = $derived(new Map(items.map((o) => [o.value, o.label])));
	const summary = $derived(value.map((v) => labelOf.get(v) ?? v).join(', '));

	const cls = $derived(
		['dbm-select', 'trigger', size === 'sm' && 'dbm-select--sm', className]
			.filter(Boolean)
			.join(' ')
	);

	function toggle(v: string, on: boolean): void {
		const next = on ? (value.includes(v) ? value : [...value, v]) : value.filter((x) => x !== v);
		value = next;
		onchange?.(next);
	}

	/** Places the menu against the trigger's offset parent rather than inside the scroll
	 * container, so an open menu neither grows that container's scroll range nor gets
	 * clipped by it. Offsets are layout pixels, which stay true under the app's zoom. It
	 * opens below the trigger unless the scroller has more room above. */
	function show(): void {
		if (disabled || !trigger) return;
		query = '';
		const parent = trigger.offsetParent as HTMLElement | null;
		let scroller: HTMLElement | null = trigger.parentElement;
		while (scroller && scroller !== parent && !/auto|scroll/.test(getComputedStyle(scroller).overflowY)) {
			scroller = scroller.parentElement;
		}
		const scrolled = scroller && scroller !== parent ? scroller.scrollTop : 0;
		const top = trigger.offsetTop - scrolled;
		const bottom = top + trigger.offsetHeight;
		let below = Infinity;
		let room = Infinity;
		if (scroller && scroller !== parent) {
			const t = trigger.getBoundingClientRect();
			const sr = scroller.getBoundingClientRect();
			below = sr.bottom - t.bottom;
			room = t.top - sr.top;
		}
		above = below < 240 && room > below;
		const height = parent?.clientHeight ?? 0;
		place = above
			? `left:${trigger.offsetLeft}px;width:${trigger.offsetWidth}px;bottom:${height - top + 4}px`
			: `left:${trigger.offsetLeft}px;width:${trigger.offsetWidth}px;top:${bottom + 4}px`;
		open = true;
	}

	function hide(): void {
		open = false;
	}

	function onTriggerKeydown(event: KeyboardEvent): void {
		if (event.key === 'ArrowDown' && !open) {
			event.preventDefault();
			show();
		}
	}

	function onWindowKeydown(event: KeyboardEvent): void {
		if (open && event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			hide();
			trigger?.focus();
		}
	}

	function onWindowPointerdown(event: PointerEvent): void {
		if (open && root && !root.contains(event.target as Node)) hide();
	}

	/** The menu is placed once on open, so a scroll underneath it closes it instead. */
	function onWindowScroll(event: Event): void {
		if (open && root && !root.contains(event.target as Node)) hide();
	}

	/** Attached to the filter box so it takes focus the moment the menu opens. */
	function focusOnMount(el: HTMLInputElement): void {
		el.focus();
	}
</script>

<svelte:window
	onpointerdown={onWindowPointerdown}
	onkeydown={onWindowKeydown}
	onscrollcapture={onWindowScroll}
/>

{#snippet control()}
	<span class="wrap" bind:this={root}>
		<button
			bind:this={trigger}
			type="button"
			id={fid}
			class={cls}
			class:placeholder={value.length === 0}
			aria-haspopup="true"
			aria-expanded={open}
			{disabled}
			data-testid={testId}
			onclick={() => (open ? hide() : show())}
			onkeydown={onTriggerKeydown}
		>
			<span class="text">{value.length === 0 ? placeholder : summary}</span>
			{#if value.length > 0}<span class="count">{value.length}</span>{/if}
			<span class="dbm-select-wrap__caret">
				<span
					style="width:0;height:0;border-left:3.5px solid transparent;border-right:3.5px solid transparent;border-top:4px solid var(--text-tertiary)"
				></span>
			</span>
		</button>
		{#if open}
			<div class="dbm-menu menu" style={place} role="group" aria-label={label ?? placeholder}>
				<span class="filter">
					<Icon name="search" size={12} />
					<input autocorrect="off" autocapitalize="off" spellcheck="false"
						{@attach focusOnMount}
						type="text"
						placeholder="Filter"
						aria-label="Filter {label ?? 'options'}"
						bind:value={query}
						data-testid={testId ? `${testId}-search` : undefined}
					/>
				</span>
				<div class="list">
					{#each rows as row (row.value)}
						<div class="dbm-menu__item option">
							<Checkbox
								label={row.label}
								checked={value.includes(row.value)}
								onchange={(e) => toggle(row.value, e.currentTarget.checked)}
								data-testid={testId ? `${testId}-${row.value}` : undefined}
							/>
						</div>
					{:else}
						<span class="empty">{emptyText}</span>
					{/each}
				</div>
			</div>
		{/if}
	</span>
{/snippet}

{#if label}
	<span class="dbm-field">
		<label class="dbm-field__label" for={fid}>{label}</label>
		{@render control()}
	</span>
{:else}
	{@render control()}
{/if}

<style>
	/* Not positioned on purpose: the menu's containing block is the nearest positioned
	   ancestor, which is the panel around the scroll container, not the scroller. */
	.wrap {
		display: block;
		width: 100%;
	}

	.trigger {
		position: relative;
		display: flex;
		align-items: center;
		gap: var(--space-2);
		text-align: left;
		cursor: pointer;
	}

	.trigger:disabled {
		cursor: default;
		opacity: 0.5;
	}

	.trigger:focus-visible {
		border-color: var(--accent);
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 0;
	}

	.text {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.placeholder .text {
		color: var(--text-tertiary);
	}

	.count {
		flex: 0 0 auto;
		min-width: 18px;
		padding: 0 5px;
		border-radius: 9px;
		background: var(--bg-overlay);
		color: var(--text-secondary);
		font-family: var(--font-mono);
		font-size: var(--mono-sm);
		line-height: 18px;
		text-align: center;
	}

	.menu {
		position: absolute;
		z-index: 20;
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		min-width: 0;
	}

	.filter {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		height: var(--h-control-sm, 24px);
		padding: 0 var(--space-2);
		border: var(--border-width) solid var(--border-default);
		border-radius: var(--radius-sm);
		background: var(--bg-base);
		color: var(--text-tertiary);
	}

	.filter input {
		flex: 1;
		min-width: 0;
		border: none;
		background: none;
		color: var(--text-primary);
		font-family: var(--font-ui);
		font-size: var(--text-sm);
		outline: none;
	}

	.filter input::placeholder {
		color: var(--text-tertiary);
	}

	.list {
		max-height: 200px;
		overflow-y: auto;
	}

	.option {
		padding: 0;
	}

	.option :global(.dbm-check) {
		width: 100%;
		padding: 4px var(--space-2);
		cursor: pointer;
	}

	.empty {
		display: block;
		padding: var(--space-2);
		color: var(--text-tertiary);
		font-size: var(--text-sm);
	}
</style>
