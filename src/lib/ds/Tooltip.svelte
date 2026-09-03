<script lang="ts">
	import type { Snippet } from 'svelte';
	import KeyHint from './KeyHint.svelte';

	interface Props {
		label: string;
		/** Logical shortcut, written with `Mod`. Shown beside the label. */
		combo?: string;
		placement?: 'top' | 'bottom';
		children?: Snippet;
	}

	let { label, combo, placement = 'top', children }: Props = $props();
</script>

<span class="dbm-tip">
	{@render children?.()}
	<span class="dbm-tip__pop dbm-tip__pop--{placement}" role="tooltip">
		{label}
		{#if combo}<span class="dbm-tip__kbd"><KeyHint {combo} plain /></span>{/if}
	</span>
</span>
