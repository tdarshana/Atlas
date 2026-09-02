<script lang="ts">
	import type { HTMLButtonAttributes } from 'svelte/elements';

	interface Props extends HTMLButtonAttributes {
		variant?: 'primary' | 'secondary' | 'danger' | 'ghost';
		size?: 'sm' | 'md';
	}

	let {
		variant = 'secondary',
		size = 'md',
		type = 'button',
		class: klass = '',
		children,
		...rest
	}: Props = $props();
</script>

<button {...rest} {type} class="btn {variant} {size} {klass}">
	{@render children?.()}
</button>

<style>
	.btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: var(--space-2);
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg-elev);
		color: var(--fg);
		font: inherit;
		font-weight: 500;
		white-space: nowrap;
		cursor: pointer;
		transition:
			background 0.12s ease,
			border-color 0.12s ease;
	}

	.md {
		padding: 7px 14px;
	}

	.sm {
		padding: 3px 9px;
		font-size: 13px;
	}

	.btn:hover:not(:disabled) {
		background: var(--bg-hover);
	}

	.primary {
		background: var(--accent);
		border-color: var(--accent);
		color: var(--accent-fg);
	}

	.primary:hover:not(:disabled) {
		background: var(--accent);
		filter: brightness(1.08);
	}

	.danger {
		background: transparent;
		border-color: var(--danger);
		color: var(--danger);
	}

	.danger:hover:not(:disabled) {
		background: var(--danger-soft);
	}

	.ghost {
		background: transparent;
		border-color: transparent;
		color: var(--muted);
	}

	.ghost:hover:not(:disabled) {
		background: var(--bg-hover);
		color: var(--fg);
	}

	.btn:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
</style>
