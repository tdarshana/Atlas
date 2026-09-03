<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';

	interface Props extends HTMLAttributes<HTMLElement> {
		/** Plain-text heading; use the `header` snippet when it needs markup. */
		title?: string;
		header?: Snippet;
		/** Right-aligned controls in the header row. */
		actions?: Snippet;
	}

	let { title, header, actions, class: klass = '', children, ...rest }: Props = $props();
</script>

<section {...rest} class="card {klass}">
	{#if header || title || actions}
		<div class="head">
			<div class="title">
				{#if header}{@render header()}{:else if title}<h2>{title}</h2>{/if}
			</div>
			{#if actions}<div class="actions">{@render actions()}</div>{/if}
		</div>
	{/if}
	<div class="body">{@render children?.()}</div>
</section>

<style>
	/* `.card` (background, border, radius) comes from `$lib/ds/index.css`. */
	.card {
		display: flex;
		flex-direction: column;
	}

	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		height: 32px;
		flex: 0 0 32px;
		padding: 0 var(--space-3);
		border-bottom: 1px solid var(--border-subtle);
	}

	.title :global(h2) {
		margin: 0;
		font-size: 13px;
		font-weight: 600;
		color: var(--text-primary);
	}

	.actions {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}

	.body {
		padding: var(--space-3);
	}
</style>
