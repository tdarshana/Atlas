<script lang="ts">
	import { copyText } from '$lib/shell';
	import { dismiss, toasts, type ToastItem } from '$lib/platform/toasts.svelte';

	/**
	 * An error interrupts whatever a screen reader is saying; a success or an info note
	 * waits its turn. The host is the live region and is always in the DOM, so the level
	 * is set on it rather than on the toasts that come and go inside it.
	 */
	const level = $derived(toasts.some((t) => t.kind === 'error') ? 'assertive' : 'polite');

	/** A toast carrying `copyText` (the export path toast) copies it on click, then
	 * dismisses as every other toast does on click. */
	async function handleClick(toast: ToastItem): Promise<void> {
		if (toast.copyText) await copyText(toast.copyText).catch(() => {});
		dismiss(toast.id);
	}
</script>

<div class="host" data-testid="toast-host" aria-live={level}>
	{#each toasts as toast (toast.id)}
		<button
			type="button"
			class="toast {toast.kind}"
			title={toast.copyText ? 'Click to copy' : undefined}
			onclick={() => handleClick(toast)}
		>
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
		border: 1px solid var(--border-default);
		border-left: 3px solid var(--text-tertiary);
		border-radius: var(--radius-md);
		background: var(--bg-raised);
		color: var(--text-primary);
		font-size: 12px;
		text-align: left;
		box-shadow: var(--shadow-md);
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
