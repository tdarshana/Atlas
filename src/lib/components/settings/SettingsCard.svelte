<script lang="ts">
	// One Settings card (frame 10): the 32px head with the title and whatever the section
	// puts beside it, then the body. A section owns its state and its own styles; this
	// owns the frame, and the `id` is the anchor `/settings#<id>` scrolls to.
	import type { Snippet } from 'svelte';

	interface Props {
		id: string;
		title: string;
		/** Rendered after the title: a badge, or a spacer and the section's buttons. */
		head?: Snippet;
		children: Snippet;
	}

	let { id, title, head, children }: Props = $props();
</script>

<section class="card" {id} data-testid="settings-section-{id}">
	<div class="card-head">
		<span class="card-title">{title}</span>
		{@render head?.()}
	</div>
	<div class="card-body">
		{@render children()}
	</div>
</section>

<style>
	.card {
		flex: 0 0 auto;
		overflow: hidden;
	}

	.card-head {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.card-title {
		font-weight: 600;
	}

	.card-body {
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
</style>
