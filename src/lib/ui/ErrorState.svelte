<script lang="ts">
	import type { Snippet } from 'svelte';
	import Icon from '$lib/ds/Icon.svelte';

	interface Props {
		message: string;
		/** Shown when the failure is a connection failure and the log may explain it. */
		logPath?: string;
		/** Optional recovery action, e.g. a Retry button. */
		children?: Snippet;
		class?: string;
	}

	let { message, logPath, children, class: klass = '' }: Props = $props();
</script>

<div class="error {klass}" role="alert" data-testid="error-state">
	<Icon name="circle-x" size={24} color="var(--danger-text)" />
	<p class="title">Something went wrong</p>
	<p class="message">{message}</p>
	{#if logPath}
		<p class="log">Log: <code>{logPath}</code></p>
	{/if}
	{@render children?.()}
</div>

<style>
	.error {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: var(--space-2);
		padding: var(--space-5) var(--space-4);
		border: 1px solid var(--border-default);
		border-radius: var(--radius-md);
		background: var(--bg-raised);
		text-align: center;
	}

	.title {
		margin: 0;
		font-weight: 600;
		color: var(--danger-text);
	}

	.message {
		margin: 0;
		max-width: 60ch;
		overflow-wrap: anywhere;
	}

	.log {
		margin: 0;
		color: var(--text-secondary);
		font-size: 13px;
	}
</style>
