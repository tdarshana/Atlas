<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLButtonAttributes } from 'svelte/elements';

	interface Props extends Omit<HTMLButtonAttributes, 'class'> {
		variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
		size?: 'md' | 'sm';
		block?: boolean;
		icon?: Snippet;
		iconRight?: Snippet;
		children?: Snippet;
		class?: string;
	}

	let {
		variant = 'secondary',
		size = 'md',
		block = false,
		icon,
		iconRight,
		children,
		class: className = '',
		...rest
	}: Props = $props();

	const cls = $derived(
		[
			'dbm-btn',
			`dbm-btn--${variant}`,
			size === 'sm' && 'dbm-btn--sm',
			block && 'dbm-btn--block',
			className
		]
			.filter(Boolean)
			.join(' ')
	);
</script>

<button type="button" {...rest} class={cls}>
	{@render icon?.()}
	{@render children?.()}
	{@render iconRight?.()}
</button>
