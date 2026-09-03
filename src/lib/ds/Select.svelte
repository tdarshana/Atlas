<script lang="ts" module>
	export type SelectOption = string | { value: string; label: string };
</script>

<script lang="ts">
	import type { HTMLSelectAttributes } from 'svelte/elements';

	// `size` is redefined: on a native select it is a row count, here it is the density step.
	interface Props extends Omit<HTMLSelectAttributes, 'class' | 'value' | 'size'> {
		options?: SelectOption[];
		size?: 'md' | 'sm';
		label?: string;
		value?: string;
		class?: string;
	}

	let {
		options = [],
		size = 'md',
		label,
		id,
		value = $bindable(''),
		class: className = '',
		...rest
	}: Props = $props();

	const generated = 'sel' + Math.random().toString(36).slice(2, 8);
	const fid = $derived(id ?? generated);

	const cls = $derived(
		['dbm-select', size === 'sm' && 'dbm-select--sm', className].filter(Boolean).join(' ')
	);

	const items = $derived(
		options.map((o) => (typeof o === 'string' ? { value: o, label: o } : o))
	);
</script>

{#snippet control()}
	<span class="dbm-select-wrap">
		<select {...rest} id={fid} class={cls} bind:value>
			{#each items as item (item.value)}
				<option value={item.value}>{item.label}</option>
			{/each}
		</select>
		<span class="dbm-select-wrap__caret">
			<span
				style="width:0;height:0;border-left:3.5px solid transparent;border-right:3.5px solid transparent;border-top:4px solid var(--text-tertiary)"
			></span>
		</span>
	</span>
{/snippet}

{#if label}
	<span class="dbm-field">
		<label class="dbm-field__label" for={fid}>{label}</label>
		{@render control()}
	</span>
{:else}
	{@render control()}
{/if}
