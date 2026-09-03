<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		open: boolean;
		title?: string;
		/** Fired for Esc, the backdrop, the close button and `open = false`. */
		onclose?: () => void;
		children?: Snippet;
		footer?: Snippet;
		class?: string;
	}

	let { open, title, onclose, children, footer, class: klass = '' }: Props = $props();

	let el = $state<HTMLDialogElement>();

	// `showModal()` is the only way to get the top layer and the backdrop, so the
	// `open` prop drives the element rather than the `open` attribute.
	$effect(() => {
		if (!el) return;
		if (open && !el.open) el.showModal();
		else if (!open && el.open) el.close();
	});
</script>

<dialog
	bind:this={el}
	class="dialog {klass}"
	data-testid="dialog"
	onclose={() => onclose?.()}
	onclick={(e) => {
		if (e.target === el) onclose?.();
	}}
>
	<div class="inner">
		<header>
			<!-- An untitled dialog gets no heading at all: an empty h2 is a heading a
			     screen reader still lands on with nothing to read out. -->
			{#if title}<h2>{title}</h2>{/if}
			<button type="button" class="x" aria-label="Close" onclick={() => onclose?.()}>×</button>
		</header>
		<div class="body">{@render children?.()}</div>
		{#if footer}<footer>{@render footer()}</footer>{/if}
	</div>
</dialog>

<style>
	.dialog {
		width: min(560px, calc(100vw - 32px));
		padding: 0;
		border: 1px solid var(--border-default);
		border-radius: var(--radius-lg);
		background: var(--bg-overlay);
		color: var(--text-primary);
		box-shadow: var(--shadow-lg);
		transition:
			opacity var(--dur-modal) var(--ease-overlay),
			transform var(--dur-modal) var(--ease-overlay);
	}

	/* Flat scrim, modals only; the palette and popovers carry none. No blur anywhere. */
	.dialog::backdrop {
		background: rgba(6, 8, 12, 0.55);
		transition: background-color var(--dur-modal) var(--ease-overlay);
	}

	@starting-style {
		.dialog[open] {
			opacity: 0;
			transform: translateY(4px);
		}
	}

	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		padding: var(--space-3) var(--space-4);
		border-bottom: 1px solid var(--border-default);
	}

	header h2 {
		margin: 0;
		font-size: 15px;
		font-weight: 600;
	}

	.x {
		/* Keeps the close button at the right edge when there is no heading beside it. */
		margin-left: auto;
		border: none;
		background: none;
		color: var(--text-secondary);
		font-size: 20px;
		line-height: 1;
		cursor: pointer;
	}

	.x:hover {
		color: var(--text-primary);
	}

	.body {
		padding: var(--space-4);
		max-height: 60vh;
		overflow-y: auto;
	}

	footer {
		display: flex;
		justify-content: flex-end;
		gap: var(--space-2);
		padding: var(--space-3) var(--space-4);
		border-top: 1px solid var(--border-default);
	}
</style>
