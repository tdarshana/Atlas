<script lang="ts">
	interface Props {
		label?: string;
		checked?: boolean;
		indeterminate?: boolean;
		disabled?: boolean;
		/** Renders the radio variant: a round box with a dot instead of a tick. */
		radio?: boolean;
		name?: string;
		value?: string;
		onchange?: (event: Event) => void;
	}

	let {
		label,
		checked = $bindable(false),
		indeterminate = false,
		disabled = false,
		radio = false,
		name,
		value,
		onchange
	}: Props = $props();

	const cls = $derived(
		['dbm-check', radio && 'dbm-radio', disabled && 'dbm-check--disabled'].filter(Boolean).join(' ')
	);
</script>

<label class={cls}>
	<input
		type={radio ? 'radio' : 'checkbox'}
		{name}
		{value}
		{checked}
		{disabled}
		onchange={(e) => {
			checked = e.currentTarget.checked;
			onchange?.(e);
		}}
	/>
	<span class="dbm-check__box{radio ? ' dbm-radio__box' : ''}">
		{#if radio}
			{#if checked}<span class="dbm-radio__dot"></span>{/if}
		{:else if indeterminate}
			<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
				<path d="M2 5h6" fill="none" stroke="#fff" stroke-width="1.6" stroke-linecap="round" />
			</svg>
		{:else if checked}
			<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
				<path
					d="M1.5 5.2 3.8 7.5 8.5 2.5"
					fill="none"
					stroke="#fff"
					stroke-width="1.6"
					stroke-linecap="round"
					stroke-linejoin="round"
				/>
			</svg>
		{/if}
	</span>
	{#if label}<span>{label}</span>{/if}
</label>
