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

<footer class="bar" class:expanded={shell.railExpanded}>
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
	/* No bar of its own: the text sits on the window background under the content panel,
	   starting where the panel starts, so the left edge follows the rail's width. The
	   right edge lines up with the panel's 6px inset. */
	.bar {
		display: flex;
		align-items: center;
		gap: 12px;
		height: 24px;
		flex: 0 0 24px;
		padding: 0 6px 0 calc(var(--w-rail) + 6px);
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.bar.expanded {
		padding-left: calc(176px + 6px);
	}

	.spacer {
		flex: 1;
	}
</style>
