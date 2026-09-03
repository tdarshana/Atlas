<script lang="ts">
	// Thin wrapper over the design system's Badge. The old tone set (neutral, accent,
	// danger, success) is a subset of the DS component's, so it maps straight through;
	// `variant` and `mono` are additive DS props no call site here uses yet.
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';
	import DsBadge from '$lib/ds/Badge.svelte';

	interface Props extends Omit<HTMLAttributes<HTMLSpanElement>, 'class'> {
		tone?: 'neutral' | 'accent' | 'danger' | 'success';
		class?: string;
		children?: Snippet;
	}

	let { tone = 'neutral', class: klass = '', children, ...rest }: Props = $props();
</script>

<DsBadge {tone} class={klass} {...rest}>
	{@render children?.()}
</DsBadge>
