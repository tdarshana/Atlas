<script module lang="ts">
	export interface SelectOption {
		value: string;
		label: string;
		/** No DS equivalent; dropped when the option is handed to the design system. */
		disabled?: boolean;
	}
</script>

<script lang="ts">
	// Thin wrapper over the design system's Select. No call site in this codebase
	// sets a per-option `disabled`, so dropping it here costs nothing today; the
	// field stays on `SelectOption` in case that changes.
	import type { HTMLSelectAttributes } from 'svelte/elements';
	import DsSelect from '$lib/ds/Select.svelte';

	// `size` is dropped: on a native select it is a row count, and the DS component
	// redefines it as a density step, which no call site here uses.
	interface Props extends Omit<HTMLSelectAttributes, 'class' | 'size'> {
		value?: string;
		options: SelectOption[];
		class?: string;
	}

	let { value = $bindable(''), options, class: klass = '', ...rest }: Props = $props();

	const dsOptions = $derived(options.map(({ value: v, label }) => ({ value: v, label })));
</script>

<DsSelect bind:value options={dsOptions} class={klass} {...rest} />
