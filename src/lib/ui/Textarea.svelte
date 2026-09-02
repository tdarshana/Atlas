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
		padding: 8px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg-elev);
		color: var(--fg);
		font: inherit;
		resize: vertical;
	}

	.textarea::placeholder {
		color: var(--muted);
	}

	.mono {
		font-family: var(--font-mono);
		font-size: 13px;
	}
</style>
