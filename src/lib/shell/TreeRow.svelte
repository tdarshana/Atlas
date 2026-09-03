<script lang="ts">
	// A 22px item row under a group: 13px icon, label that ellipses, mono meta on the right.
	// Selection is the accent-muted fill plus the 2px inset accent edge the system uses for
	// every selected nav row.
	import { Icon, type IconName } from '$lib/ds';

	interface Props {
		icon: IconName;
		label: string;
		/** Mono for names the system produced: project names, tags, hosts. */
		mono?: boolean;
		meta?: number | string;
		selected?: boolean;
		iconColor?: string;
		/** For a row that is a destination; `onclick` is for a row that is an action. */
		href?: string;
		/** Also runs on a row that has an `href`, for a link whose target is already open. */
		onclick?: () => void;
	}

	let {
		icon,
		label,
		mono = false,
		meta,
		selected = false,
		iconColor,
		href,
		onclick
	}: Props = $props();
</script>

{#snippet body()}
	<Icon
		name={icon}
		size={13}
		color={iconColor ?? (selected ? 'var(--accent)' : 'var(--text-tertiary)')}
	/>
	<span class="label" class:mono>{label}</span>
	<span class="spacer"></span>
	{#if meta !== undefined}<span class="meta">{meta}</span>{/if}
{/snippet}

{#if href}
	<a class="row" class:selected {href} aria-current={selected ? 'true' : undefined} {onclick}>
		{@render body()}
	</a>
{:else if onclick}
	<button class="row" class:selected type="button" aria-current={selected ? 'true' : undefined} {onclick}>
		{@render body()}
	</button>
{:else}
	<!-- A row with no action is a reading, not a control. -->
	<div class="row static" class:selected>{@render body()}</div>
{/if}

<style>
	.row {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 22px;
		flex: 0 0 22px;
		width: 100%;
		padding: 0 8px 0 22px;
		border: 0;
		background: transparent;
		color: var(--text-secondary);
		font-family: inherit;
		font-size: 12px;
		text-align: left;
		text-decoration: none;
		cursor: default;
		transition: var(--transition-hover);
	}

	.row:hover:not(.static) {
		background: var(--bg-hover);
		color: var(--text-primary);
	}

	.row.selected {
		background: var(--accent-muted);
		color: var(--text-primary);
		box-shadow: inset 2px 0 0 var(--accent);
	}

	.label {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.label.mono {
		font-family: var(--font-mono);
	}

	.spacer {
		flex: 1;
	}

	.meta {
		font-family: var(--font-mono);
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
