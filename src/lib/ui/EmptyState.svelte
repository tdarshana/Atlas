<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		title: string;
		hint?: string;
		/** Optional call to action, e.g. a Button. */
		children?: Snippet;
		class?: string;
	}

	let { title, hint, children, class: klass = '' }: Props = $props();
</script>

<div class="empty {klass}" data-testid="empty-state">
	<p class="title">{title}</p>
	{#if hint}<p class="hint">{hint}</p>{/if}
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

	.title {
		margin: 0;
		font-weight: 600;
	}

	.hint {
		margin: 0;
		max-width: 46ch;
		color: var(--muted);
	}
</style>
