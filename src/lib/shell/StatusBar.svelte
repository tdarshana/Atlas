<script lang="ts">
	// The 24px foot of the window: where the daemon is and what it runs on the left, the
	// active page's counts on the right.
	import { daemon } from '$lib/daemon.svelte';
	import { status } from '$lib/stores/status.svelte';
	import { shell } from './shell.svelte';

	const version = $derived(status.report ? `v${status.report.version}` : '');

	const TONES: Record<string, string> = {
		success: 'var(--success-text)',
		warning: 'var(--warning-text)',
		danger: 'var(--danger-text)'
	};
</script>

<footer class="bar">
	<span class="mono">daemon 127.0.0.1:{daemon.port}</span>
	{#if version}<span class="mono">{version}</span>{/if}
	{#each shell.statusItems.left as item, i (i)}
		<span>{item}</span>
	{/each}
	<span class="spacer"></span>
	{#each shell.statusItems.right as item, i (i)}
		<span class="mono" style:color={item.tone ? TONES[item.tone] : undefined}>{item.text}</span>
	{/each}
</footer>

<style>
	.bar {
		display: flex;
		align-items: center;
		gap: 12px;
		height: 24px;
		flex: 0 0 24px;
		padding: 0 10px;
		background: var(--bg-surface);
		border-top: 1px solid var(--border-default);
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.spacer {
		flex: 1;
	}
</style>
