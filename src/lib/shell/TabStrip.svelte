<script lang="ts">
	// The 34px view tab strip. Items carry an href when the tabs are routes and fall back to
	// `onselect` when they only switch a pane.
	import { Icon, type IconName } from '$lib/ds';

	export interface Tab {
		id: string;
		label: string;
		icon?: IconName;
		href?: string;
	}

	interface Props {
		items: Tab[];
		active: string;
		onselect?: (id: string) => void;
	}

	let { items, active, onselect }: Props = $props();
</script>

<div class="tab-strip" role="tablist">
	{#each items as tab (tab.id)}
		{@const selected = tab.id === active}
		{#if tab.href}
			<a class="tab-strip__item" role="tab" aria-selected={selected} href={tab.href}>
				{#if tab.icon}
					<Icon
						name={tab.icon}
						size={13}
						color={selected ? 'var(--accent)' : 'var(--text-tertiary)'}
					/>
				{/if}
				{tab.label}
			</a>
		{:else}
			<button
				class="tab-strip__item"
				type="button"
				role="tab"
				aria-selected={selected}
				onclick={() => onselect?.(tab.id)}
			>
				{#if tab.icon}
					<Icon
						name={tab.icon}
						size={13}
						color={selected ? 'var(--accent)' : 'var(--text-tertiary)'}
					/>
				{/if}
				{tab.label}
			</button>
		{/if}
	{/each}
</div>

<style>
	a.tab-strip__item {
		text-decoration: none;
	}
</style>
