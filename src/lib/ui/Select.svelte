<script module lang="ts">
	export interface SelectOption {
		value: string;
		label: string;
		disabled?: boolean;
	}
</script>

<script lang="ts">
	import type { HTMLSelectAttributes } from 'svelte/elements';

	interface Props extends HTMLSelectAttributes {
		value?: string;
		options: SelectOption[];
	}

	let { value = $bindable(''), options, class: klass = '', ...rest }: Props = $props();
</script>

<select {...rest} class="select {klass}" bind:value>
	{#each options as option (option.value)}
		<option value={option.value} disabled={option.disabled}>{option.label}</option>
	{/each}
</select>

<style>
	.select {
		width: 100%;
		padding: 7px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg-elev);
		color: var(--fg);
		font: inherit;
	}

	.select:disabled {
		opacity: 0.6;
	}
</style>
