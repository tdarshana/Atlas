<script lang="ts">
	import { toasts, dismiss } from './toasts.svelte';
</script>

<div class="host" data-testid="toast-host" aria-live="polite">
	{#each toasts as toast (toast.id)}
		<button type="button" class="toast {toast.kind}" onclick={() => dismiss(toast.id)}>
			{toast.text}
		</button>
	{/each}
</div>

<style>
	.host {
		position: fixed;
		right: var(--space-4);
		bottom: var(--space-4);
		z-index: 100;
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		pointer-events: none;
	}

	.toast {
		pointer-events: auto;
		max-width: 360px;
		padding: var(--space-2) var(--space-3);
		border: 1px solid var(--border);
		border-left: 3px solid var(--muted);
		border-radius: var(--radius);
		background: var(--bg-elev);
		color: var(--fg);
		font: inherit;
		text-align: left;
		box-shadow: var(--shadow);
		cursor: pointer;
	}

	.success {
		border-left-color: var(--success);
	}

	.error {
		border-left-color: var(--danger);
	}

	.info {
		border-left-color: var(--accent);
	}
</style>
