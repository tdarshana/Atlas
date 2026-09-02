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
			<h2>{title ?? ''}</h2>
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
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg-elev);
		color: var(--fg);
		box-shadow: var(--shadow);
	}

	.dialog::backdrop {
		background: rgba(0, 0, 0, 0.5);
	}

	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		padding: var(--space-3) var(--space-4);
		border-bottom: 1px solid var(--border);
	}

	header h2 {
		margin: 0;
	}

	.x {
		border: none;
		background: none;
		color: var(--muted);
		font-size: 20px;
		line-height: 1;
		cursor: pointer;
	}

	.x:hover {
		color: var(--fg);
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
		border-top: 1px solid var(--border);
	}
</style>
