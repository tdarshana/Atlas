<script lang="ts">
	// A select drawn as a menu rather than the native control, so each option can carry
	// a mark: a kind square, a coloured glyph, or a plain label. The trigger wears the
	// select's clothes and shows the current option; `searchable` adds a filter box at
	// the top of the menu for long lists such as a roster.
	import { Icon, IconButton, type IconName } from '$lib/ds';
	import KindIcon from './KindIcon.svelte';

	export interface MenuOption {
		value: string;
		label: string;
		/** A kind square for this option. */
		kind?: string;
		/** A glyph in `color` for this option. */
		icon?: IconName;
		color?: string;
		/** A second, dimmer line of text. */
		hint?: string;
		/** A `font-family` list to draw this option\'s label in, for a font picker. */
		font?: string;
	}

	interface Props {
		value: string;
		options: MenuOption[];
		label?: string;
		placeholder?: string;
		searchable?: boolean;
		onchange?: (value: string) => void;
		testId?: string;
		/** A reset arrow after the label while the value differs from its default. */
		onreset?: () => void;
	}

	let { value, options, label, placeholder = 'Select…', searchable = false, onchange, testId, onreset }: Props = $props();

	let open = $state(false);
	let query = $state('');
	let root = $state<HTMLElement>();
	const uid = $props.id();

	const current = $derived(options.find((o) => o.value === value));
	const rows = $derived.by(() => {
		const q = query.trim().toLowerCase();
		return q === ''
			? options
			: options.filter((o) => o.label.toLowerCase().includes(q) || (o.hint ?? '').toLowerCase().includes(q));
	});

	function show(): void {
		query = '';
		open = true;
	}

	function pick(v: string): void {
		open = false;
		if (v !== value) onchange?.(v);
	}

	function onWindowPointerdown(event: PointerEvent): void {
		if (open && root && !root.contains(event.target as Node)) open = false;
	}

	function onWindowKeydown(event: KeyboardEvent): void {
		if (open && event.key === 'Escape') open = false;
	}

	function focusOnMount(el: HTMLInputElement): void {
		el.focus();
	}
</script>

<svelte:window onpointerdown={onWindowPointerdown} onkeydown={onWindowKeydown} />

{#snippet mark(o: MenuOption, size: number)}
	{#if o.kind}
		<KindIcon kind={o.kind} {size} />
	{:else if o.icon}
		<Icon name={o.icon} {size} color={o.color ?? 'currentColor'} />
	{/if}
{/snippet}

<span class="dbm-field">
	{#if label}
		<span class="dbm-field__labelrow">
			<label class="dbm-field__label" for={uid}>{label}</label>
			{#if onreset}
				<IconButton size="sm" icon="undo-2" label="Reset {label}" data-testid={testId ? `${testId}-reset` : undefined} onclick={onreset} />
			{/if}
		</span>
	{/if}
	<!-- Inline on purpose: the menu anchors to this span, and the scoped rule was seen
	     losing to the field's own layout in the app. -->
	<span class="wrap" style="position:relative" bind:this={root}>
		<button
			type="button"
			id={uid}
			class="dbm-select trigger"
			class:placeholder={!current}
			aria-haspopup="menu"
			aria-expanded={open}
			data-testid={testId}
			onclick={() => (open ? (open = false) : show())}
		>
			{#if current}{@render mark(current, 14)}{/if}
			<span class="text">{current?.label ?? placeholder}</span>
			<span class="dbm-select-wrap__caret">
				<span
					style="width:0;height:0;border-left:3.5px solid transparent;border-right:3.5px solid transparent;border-top:4px solid var(--text-tertiary)"
				></span>
			</span>
		</button>
		{#if open}
			<div class="dbm-menu menu" role="menu" data-testid={testId ? `${testId}-menu` : undefined}>
				{#if searchable}
					<span class="filter">
						<Icon name="search" size={12} />
						<input
							{@attach focusOnMount}
							type="text"
							placeholder="Filter"
							autocomplete="off"
							autocorrect="off"
							autocapitalize="off"
							spellcheck="false"
							aria-label="Filter {label ?? 'options'}"
							bind:value={query}
							data-testid={testId ? `${testId}-search` : undefined}
						/>
					</span>
				{/if}
				<div class="list">
					{#each rows as o (o.value)}
						<button
							type="button"
							class="dbm-menu__item item"
							role="menuitemradio"
							aria-checked={o.value === value}
							data-testid={testId ? `${testId}-${o.value || 'none'}` : undefined}
							onclick={() => pick(o.value)}
						>
							{@render mark(o, 14)}
							<span class="text" style={o.font ? `font-family:${o.font}` : undefined}>
								{o.label}
								{#if o.hint}<span class="hint">{o.hint}</span>{/if}
							</span>
							{#if o.value === value}<Icon name="check" size={12} />{/if}
						</button>
					{:else}
						<span class="empty">Nothing matches.</span>
					{/each}
				</div>
			</div>
		{/if}
	</span>
</span>

<style>
	.wrap {
		position: relative;
		display: block;
		width: 100%;
	}

	.trigger {
		position: relative;
		display: flex;
		align-items: center;
		gap: 8px;
		text-align: left;
		cursor: pointer;
	}

	.trigger:focus-visible {
		border-color: var(--accent);
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 0;
	}

	.placeholder .text {
		color: var(--text-tertiary);
	}

	.text {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.hint {
		margin-left: 6px;
		color: var(--text-tertiary);
		font-size: 11px;
	}

	.menu {
		position: absolute;
		left: 0;
		right: 0;
		top: calc(100% + 4px);
		z-index: 30;
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		min-width: 0;
	}

	.filter {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		height: 24px;
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

	.list {
		max-height: 220px;
		overflow-y: auto;
	}

	.item {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		text-align: left;
	}

	.empty {
		display: block;
		padding: var(--space-2);
		color: var(--text-tertiary);
		font-size: var(--text-sm);
	}
</style>
