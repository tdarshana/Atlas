<script module lang="ts">
	export interface TabItem {
		id: string;
		label: string;
		disabled?: boolean;
	}
</script>

<script lang="ts">
	interface Props {
		items: TabItem[];
		active: string;
		onchange: (id: string) => void;
		class?: string;
	}

	let { items, active, onchange, class: klass = '' }: Props = $props();
</script>

<div class="tabs {klass}" role="tablist">
	{#each items as item (item.id)}
		<button
			type="button"
			role="tab"
			class="tab"
			class:active={item.id === active}
			aria-selected={item.id === active}
			disabled={item.disabled}
			data-testid="tab-{item.id}"
			onclick={() => onchange(item.id)}
		>
			{item.label}
		</button>
	{/each}
</div>

<style>
	.tabs {
		display: flex;
		gap: var(--space-1);
		border-bottom: 1px solid var(--border);
	}

	.tab {
		padding: 6px 12px;
		border: none;
		border-bottom: 2px solid transparent;
		margin-bottom: -1px;
		background: none;
		color: var(--muted);
		font: inherit;
		cursor: pointer;
	}

	.tab:hover:not(:disabled) {
		color: var(--fg);
	}

	.tab.active {
		color: var(--fg);
		border-bottom-color: var(--accent);
	}

	.tab:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
</style>
