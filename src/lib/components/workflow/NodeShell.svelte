<script lang="ts">
	// The card chrome shared by the three node kinds (frame 08): a 28px header with an
	// icon and a title, then a body of 18px rows the caller supplies. Selection is a 1px
	// accent border plus the raised shadow, same rule as everywhere else selection shows.
	import type { Snippet } from 'svelte';
	import { Icon, type IconName } from '$lib/ds';

	interface Props {
		width: number;
		selected: boolean;
		icon: IconName;
		iconColor?: string;
		title: string;
		headerRight?: Snippet;
		children?: Snippet;
	}

	let { width, selected, icon, iconColor, title, headerRight, children }: Props = $props();
</script>

<div class="card" class:selected style="width:{width}px">
	<div class="header">
		<Icon name={icon} size={13} color={iconColor ?? 'var(--text-tertiary)'} />
		<span class="title">{title}</span>
		{#if headerRight}{@render headerRight()}{/if}
	</div>
	<div class="body">
		{@render children?.()}
	</div>
</div>

<style>
	.card {
		display: flex;
		flex-direction: column;
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
		box-shadow: var(--shadow-sm);
	}

	.card.selected {
		border-color: var(--accent);
		box-shadow: var(--shadow-md);
	}

	.header {
		height: 28px;
		flex: 0 0 28px;
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 0 10px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.title {
		flex: 1;
		min-width: 0;
		font-weight: 600;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.body {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 6px 10px;
		color: var(--text-secondary);
	}
</style>
