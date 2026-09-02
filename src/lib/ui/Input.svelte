<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';

	interface Props extends HTMLInputAttributes {
		value?: string;
	}

	let { value = $bindable(''), class: klass = '', ...rest }: Props = $props();
</script>

<input {...rest} class="input {klass}" bind:value />

<style>
	.input {
		width: 100%;
		padding: 7px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg-elev);
		color: var(--fg);
		font: inherit;
	}

	.input::placeholder {
		color: var(--muted);
	}

	.input:disabled {
		opacity: 0.6;
	}
</style>
