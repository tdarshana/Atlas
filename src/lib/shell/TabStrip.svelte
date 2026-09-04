<script lang="ts">
	// The 34px view tab strip. Items carry an href when the tabs are routes and fall back to
	// `onselect` when they only switch a pane.
	import { Badge, Icon, type IconName } from '$lib/ds';

	export interface Tab {
		id: string;
		label: string;
		icon?: IconName;
		href?: string;
		/** Shown as a small outline badge after the label when greater than zero. */
		count?: number;
	}

	interface Props {
		items: Tab[];
		active: string;
		onselect?: (id: string) => void;
		/** Set to stamp `data-testid="<testid>-<tab.id>"` on each tab button. */
		testid?: string;
	}

	let { items, active, onselect, testid }: Props = $props();
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
				{#if typeof tab.count === 'number' && tab.count > 0}
					<Badge variant="outline">{tab.count}</Badge>
				{/if}
			</a>
		{:else}
			<button
				class="tab-strip__item"
				type="button"
				role="tab"
				aria-selected={selected}
				data-testid={testid ? `${testid}-${tab.id}` : undefined}
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
				{#if typeof tab.count === 'number' && tab.count > 0}
					<Badge variant="outline">{tab.count}</Badge>
				{/if}
			</button>
		{/if}
	{/each}
</div>

<style>
	a.tab-strip__item {
		text-decoration: none;
	}
</style>
