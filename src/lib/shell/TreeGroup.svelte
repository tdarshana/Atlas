<script lang="ts">
	// A 22px disclosure heading inside the side panel: chevron, uppercase label, mono count.
	import { untrack, type Snippet } from 'svelte';
	import { Icon } from '$lib/ds';

	interface Props {
		label: string;
		/** Omit for a group whose size is not worth stating. */
		count?: number | string;
		/** Disclosure is the row's own business once it is drawn. */
		initialOpen?: boolean;
		children?: Snippet;
	}

	let { label, count, initialOpen = true, children }: Props = $props();

	// Deliberately the initial value only: disclosure is the row's state after that.
	let open = $state(untrack(() => initialOpen));
</script>

<button class="group" type="button" aria-expanded={open} onclick={() => (open = !open)}>
	<Icon name={open ? 'chevron-down' : 'chevron-right'} size={12} />
	<span class="group-heading name">{label}</span>
	<span class="spacer"></span>
	{#if count !== undefined}<span class="count">{count}</span>{/if}
</button>
{#if open}{@render children?.()}{/if}

<style>
	.group {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 22px;
		flex: 0 0 22px;
		width: 100%;
		padding: 0 8px;
		border: 0;
		background: transparent;
		color: var(--text-secondary);
		font-family: inherit;
		font-size: 12px;
		text-align: left;
		cursor: default;
	}

	.name {
		color: var(--text-primary);
	}

	.spacer {
		flex: 1;
	}

	/* A count is short, but the Config group puts a filesystem path here. It ellipses
	   rather than pushing the panel into a horizontal scrollbar. */
	.count {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
