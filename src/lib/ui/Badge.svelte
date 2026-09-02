<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';

	interface Props extends HTMLAttributes<HTMLSpanElement> {
		tone?: 'neutral' | 'accent' | 'danger' | 'success';
	}

	let { tone = 'neutral', class: klass = '', children, ...rest }: Props = $props();
</script>

<span {...rest} class="badge {tone} {klass}">{@render children?.()}</span>

<style>
	.badge {
		display: inline-flex;
		align-items: center;
		gap: var(--space-1);
		padding: 1px 8px;
		border: 1px solid var(--border);
		border-radius: 999px;
		font-size: 12px;
		line-height: 18px;
		white-space: nowrap;
		color: var(--muted);
	}

	.accent {
		border-color: transparent;
		background: var(--accent-soft);
		color: var(--accent);
	}

	.danger {
		border-color: transparent;
		background: var(--danger-soft);
		color: var(--danger);
	}

	.success {
		border-color: transparent;
		background: var(--success-soft);
		color: var(--success);
	}
</style>
