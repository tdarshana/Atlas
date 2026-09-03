<script lang="ts">
	import type { Snippet } from 'svelte';
	import Icon from './Icon.svelte';

	interface Props {
		tone?: 'neutral' | 'accent' | 'success' | 'warning' | 'info' | 'danger';
		variant?: 'soft' | 'solid' | 'outline';
		/** Mono for keys, tags and counts. */
		mono?: boolean;
		/** Icon name. Colour is never the only carrier, so state badges pair with a glyph. */
		icon?: string;
		children?: Snippet;
		class?: string;
	}

	let {
		tone = 'neutral',
		variant = 'soft',
		mono = false,
		icon,
		children,
		class: className = ''
	}: Props = $props();

	const key = $derived(
		variant === 'solid' && (tone === 'danger' || tone === 'accent')
			? `solid-${tone}`
			: variant === 'outline'
				? 'outline'
				: tone
	);

	const cls = $derived(
		['dbm-badge', `dbm-badge--${key}`, mono && 'dbm-badge--mono', className].filter(Boolean).join(' ')
	);
</script>

<span class={cls}>
	{#if icon}<Icon name={icon} size={10} />{/if}
	{@render children?.()}
</span>
