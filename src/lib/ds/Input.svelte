<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';
	import Icon from './Icon.svelte';
	import IconButton from './IconButton.svelte';

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
		/** When given, a reset arrow follows the label; the caller passes it only while the
		 * value differs from its default, and it puts the default back. */
		onreset?: () => void;
	}

	let {
		label,
		hint,
		error,
		mono = false,
		icon,
		id,
		onreset,
		value = $bindable(''),
		class: className = '',
		...rest
	}: Props = $props();

	const uid = $props.id();
	const fid = $derived(id ?? uid);
	const testid = $derived((rest as Record<string, unknown>)['data-testid'] as string | undefined);

	const cls = $derived(
		['dbm-input', mono && 'dbm-input--mono', error && 'dbm-input--invalid', className]
			.filter(Boolean)
			.join(' ')
	);

	/* The element's own handler comes after {...rest}, so it would otherwise replace the
	   caller's. Update the binding, then hand the same event on. */
	function oninput(event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
		value = event.currentTarget.value;
		rest.oninput?.(event);
	}
</script>

{#snippet control()}
	<input {...rest} id={fid} class={cls} aria-invalid={error ? true : undefined} {value} {oninput} />
{/snippet}

{#snippet field()}
	{#if icon}
		<span class="dbm-input-wrap">
			<span class="dbm-input-wrap__icon"><Icon name={icon} size={12} /></span>
			{@render control()}
		</span>
	{:else}
		{@render control()}
	{/if}
{/snippet}

{#if label || hint || error}
	<span class="dbm-field">
		{#if label}
			<span class="dbm-field__labelrow">
				<label class="dbm-field__label" for={fid}>{label}</label>
				{#if onreset}
					<IconButton size="sm" icon="undo-2" label="Reset {label}" data-testid={testid ? `${testid}-reset` : undefined} onclick={onreset} />
				{/if}
			</span>
		{/if}
		{@render field()}
		{#if error || hint}
			<span class="dbm-field__hint{error ? ' dbm-field__hint--error' : ''}">{error || hint}</span>
		{/if}
	</span>
{:else}
	{@render field()}
{/if}
