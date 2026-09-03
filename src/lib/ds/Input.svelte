<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';
	import Icon from './Icon.svelte';

	interface Props extends Omit<HTMLInputAttributes, 'class' | 'value'> {
		label?: string;
		hint?: string;
		/** Replaces the hint and marks the field invalid. */
		error?: string;
		/** Mono for anything the system or the user's data produced: paths, keys, hosts. */
		mono?: boolean;
		/** Icon name rendered inside the field. */
		icon?: string;
		value?: string;
		class?: string;
	}

	let {
		label,
		hint,
		error,
		mono = false,
		icon,
		id,
		value = $bindable(''),
		class: className = '',
		...rest
	}: Props = $props();

	const generated = 'in' + Math.random().toString(36).slice(2, 8);
	const fid = $derived(id ?? generated);

	const cls = $derived(
		['dbm-input', mono && 'dbm-input--mono', error && 'dbm-input--invalid', className]
			.filter(Boolean)
			.join(' ')
	);
</script>

{#snippet field()}
	{#if icon}
		<span class="dbm-input-wrap">
			<span class="dbm-input-wrap__icon"><Icon name={icon} size={12} /></span>
			<input
				{...rest}
				id={fid}
				class={cls}
				aria-invalid={error ? true : undefined}
				{value}
				oninput={(e) => (value = e.currentTarget.value)}
			/>
		</span>
	{:else}
		<input
			{...rest}
			id={fid}
			class={cls}
			aria-invalid={error ? true : undefined}
			{value}
			oninput={(e) => (value = e.currentTarget.value)}
		/>
	{/if}
{/snippet}

{#if label || hint || error}
	<span class="dbm-field">
		{#if label}<label class="dbm-field__label" for={fid}>{label}</label>{/if}
		{@render field()}
		{#if error || hint}
			<span class="dbm-field__hint{error ? ' dbm-field__hint--error' : ''}">{error || hint}</span>
		{/if}
	</span>
{:else}
	{@render field()}
{/if}
