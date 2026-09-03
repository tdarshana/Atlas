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

<div class="tab-strip {klass}" role="tablist">
	{#each items as item (item.id)}
		<button
			type="button"
			role="tab"
			class="tab-strip__item"
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
	/* `.tab-strip`/`.tab-strip__item` (34px, 2px inset accent underline on the active
	   tab) come from `$lib/ds/index.css`; only the disabled state is added here. */
	.tab-strip__item:disabled {
		opacity: 0.4;
		pointer-events: none;
	}
</style>
