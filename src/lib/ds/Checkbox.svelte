<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';
	import IconButton from './IconButton.svelte';

	// Rest props land on the input, as in the React original.
	interface Props extends Omit<HTMLInputAttributes, 'type' | 'checked'> {
		label?: string;
		checked?: boolean;
		indeterminate?: boolean;
		/** Renders the radio variant: a round box with a dot instead of a tick. */
		radio?: boolean;
		/** When given, a reset arrow follows the label; the caller passes it only while the
		 * value differs from its default, and it puts the default back. */
		onreset?: () => void;
	}

	let {
		label,
		checked = $bindable(false),
		indeterminate = false,
		disabled = false,
		radio = false,
		onreset,
		...rest
	}: Props = $props();

	/* Our handler comes after {...rest}, so call the caller's through. */
	function onchange(event: Event & { currentTarget: EventTarget & HTMLInputElement }) {
		checked = event.currentTarget.checked;
		rest.onchange?.(event);
	}

	const cls = $derived(
		['dbm-check', radio && 'dbm-radio', disabled && 'dbm-check--disabled'].filter(Boolean).join(' ')
	);
</script>

<label class={cls}>
	<input {...rest} type={radio ? 'radio' : 'checkbox'} {checked} {disabled} {onchange} />
	<span class="dbm-check__box{radio ? ' dbm-radio__box' : ''}">
		{#if radio}
			{#if checked}<span class="dbm-radio__dot"></span>{/if}
		{:else if indeterminate}
			<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
				<path d="M2 5h6" fill="none" stroke="var(--text-on-accent)" stroke-width="1.6" stroke-linecap="round" />
			</svg>
		{:else if checked}
			<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
				<path
					d="M1.5 5.2 3.8 7.5 8.5 2.5"
					fill="none"
					stroke="var(--text-on-accent)"
					stroke-width="1.6"
					stroke-linecap="round"
					stroke-linejoin="round"
				/>
			</svg>
		{/if}
	</span>
	{#if label}<span>{label}</span>{/if}
	{#if onreset}
		<!-- A button is interactive content, so the label does not toggle the box for it. -->
		<IconButton
			size="sm"
			icon="undo-2"
			label="Reset {label ?? 'setting'}"
			data-testid={(rest as Record<string, unknown>)['data-testid'] ? `${(rest as Record<string, unknown>)['data-testid']}-reset` : undefined}
			onclick={(e) => {
				e.preventDefault();
				e.stopPropagation();
				onreset();
			}}
		/>
	{/if}
</label>
