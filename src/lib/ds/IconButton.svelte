<script lang="ts">
	import type { HTMLButtonAttributes } from 'svelte/elements';
	import Icon from './Icon.svelte';

	interface Props extends Omit<HTMLButtonAttributes, 'class'> {
		/** Icon name from the design file's Lucide vocabulary. */
		icon: string;
		/** Names the action. Used for both `aria-label` and the native tooltip. */
		label: string;
		size?: 'md' | 'sm';
		active?: boolean;
		tone?: 'default' | 'danger';
		class?: string;
	}

	let {
		icon,
		label,
		size = 'md',
		active = false,
		tone = 'default',
		class: className = '',
		...rest
	}: Props = $props();

	const cls = $derived(
		[
			'dbm-iconbtn',
			size === 'sm' && 'dbm-iconbtn--sm',
			active && 'dbm-iconbtn--active',
			tone === 'danger' && 'dbm-iconbtn--danger',
			className
		]
			.filter(Boolean)
			.join(' ')
	);
</script>

<button
	type="button"
	{...rest}
	class={cls}
	aria-label={label}
	title={label}
	aria-pressed={active ? true : undefined}
>
	<Icon name={icon} size={size === 'sm' ? 12 : 14} />
</button>
