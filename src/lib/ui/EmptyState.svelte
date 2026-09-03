<script lang="ts">
	import type { Snippet } from 'svelte';
	import Icon from '$lib/ds/Icon.svelte';

	interface Props {
		title: string;
		/** Kept for callers that still pass one; folded into a single line under the title. */
		hint?: string;
		/** Optional call to action, e.g. a Button. */
		children?: Snippet;
		class?: string;
	}

	let { title, hint, children, class: klass = '' }: Props = $props();
</script>

<div class="empty {klass}" data-testid="empty-state">
	<Icon name="info" size={24} color="var(--text-tertiary)" />
	<p class="line">{title}{#if hint}<span class="hint"> · {hint}</span>{/if}</p>
	{@render children?.()}
</div>

<style>
	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: var(--space-2);
		padding: var(--space-6) var(--space-4);
		text-align: center;
	}

	.line {
		margin: 0;
		color: var(--text-tertiary);
		font-size: var(--text-sm);
	}

	.hint {
		color: var(--text-tertiary);
	}
</style>
