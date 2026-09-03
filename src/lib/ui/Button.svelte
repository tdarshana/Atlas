<script lang="ts">
	// Thin wrapper over the design system's Button so the pre-shell screens compile
	// unchanged while picking up its look. The prop surface is a subset of the DS
	// component's, so nothing here maps or drops anything.
	import type { Snippet } from 'svelte';
	import type { HTMLButtonAttributes } from 'svelte/elements';
	import DsButton from '$lib/ds/Button.svelte';

	interface Props extends Omit<HTMLButtonAttributes, 'class'> {
		variant?: 'primary' | 'secondary' | 'danger' | 'ghost';
		size?: 'sm' | 'md';
		class?: string;
		children?: Snippet;
	}

	let {
		variant = 'secondary',
		size = 'md',
		class: klass = '',
		children,
		...rest
	}: Props = $props();
</script>

<DsButton {variant} {size} class={klass} {...rest}>
	{@render children?.()}
</DsButton>
