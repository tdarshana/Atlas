<script lang="ts">
	import type { HTMLTextareaAttributes } from 'svelte/elements';

	interface Props extends HTMLTextareaAttributes {
		value?: string;
		mono?: boolean;
	}

	let { value = $bindable(''), mono = false, rows = 6, class: klass = '', ...rest }: Props = $props();
</script>

<textarea {...rest} {rows} class="textarea {klass}" class:mono bind:value></textarea>

<style>
	.textarea {
		width: 100%;
		padding: var(--space-2);
		border: var(--border-width) solid var(--border-default);
		border-radius: var(--radius-sm);
		background: var(--bg-base);
		color: var(--text-primary);
		font-family: var(--font-ui);
		font-size: var(--text-sm);
		resize: vertical;
		transition: var(--transition-hover);
	}

	.textarea::placeholder {
		color: var(--text-tertiary);
	}

	.textarea:hover {
		border-color: var(--border-strong);
	}

	.textarea:focus {
		border-color: var(--accent);
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 0;
	}

	.textarea:disabled {
		opacity: 0.5;
		background: var(--bg-surface);
	}

	.mono {
		font-family: var(--font-mono);
		font-size: var(--mono-sm);
		font-variant-numeric: var(--tabular);
	}
</style>
