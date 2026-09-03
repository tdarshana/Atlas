<script lang="ts">
	// An inset surface on the window background, with the standard 32px header when one is
	// given. Panels never scroll as a whole; their bodies do.
	import type { Snippet } from 'svelte';

	interface Props {
		header?: Snippet;
		children: Snippet;
		class?: string;
	}

	let { header, children, class: klass = '' }: Props = $props();
</script>

<section class="panel {klass}">
	{#if header}
		<div class="head">{@render header()}</div>
	{/if}
	{@render children()}
</section>

<style>
	.head {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 10px;
		font-weight: 600;
	}
</style>
