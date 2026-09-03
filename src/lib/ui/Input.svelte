<script lang="ts">
	// Thin wrapper over the design system's Input. The old call sites only ever bind
	// `value` and pass native input attributes; the DS component's extra props
	// (label, hint, error, mono, icon) are additive and simply go unused here.
	import type { HTMLInputAttributes } from 'svelte/elements';
	import DsInput from '$lib/ds/Input.svelte';

	interface Props extends Omit<HTMLInputAttributes, 'class'> {
		value?: string;
		class?: string;
	}

	let { value = $bindable(''), class: klass = '', ...rest }: Props = $props();
</script>

<DsInput bind:value class={klass} {...rest} />
