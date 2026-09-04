<script lang="ts">
	// The 220px contextual panel next to the rail. Its body is per view; the header and the
	// footer line are the same everywhere, so they live here.
	import { Icon } from '$lib/ds';
	import { daemon } from '$lib/daemon.svelte';
	import { status, statusLabel } from '$lib/stores/status.svelte';
	import { SIDEPANEL_MAX, SIDEPANEL_MIN, setSidePanelWidth, shell } from './shell.svelte';
	import ResizeBar from '$lib/ui/ResizeBar.svelte';
	import Projects from './sidepanels/Projects.svelte';
	import Memories from './sidepanels/Memories.svelte';
	import Agents from './sidepanels/Agents.svelte';
	import Practices from './sidepanels/Practices.svelte';
	import Workflows from './sidepanels/Workflows.svelte';
	import Review from './sidepanels/Review.svelte';
	import Mcp from './sidepanels/Mcp.svelte';
	import Settings from './sidepanels/Settings.svelte';

	const failed = $derived(!!daemon.error || !!status.error);
	const footer = $derived(failed ? (daemon.error ?? status.error ?? '') : statusLabel());

	// A page can lend the panel its own body, as the Board tab does with its filters.
	let node = $state<HTMLElement>();
	const override = $derived(shell.sidePanelOverride);
	const title = $derived(override?.title ?? shell.sidePanelTitle);
</script>

<aside
	bind:this={node}
	class="panel side"
	style="--side-w:{shell.sidePanelWidth}px"
	aria-label={title}
>
	<div class="head">
		<span class="title">{title}</span>
		<span class="spacer"></span>
		<Icon name="ellipsis" size={13} color="var(--text-tertiary)" />
	</div>

	<div class="body">
		{#if override}
			{@const Body = override.component}
			<Body />
		{:else if shell.view === 'projects'}
			<Projects />
		{:else if shell.view === 'memories'}
			<Memories />
		{:else if shell.view === 'agents'}
			<Agents />
		{:else if shell.view === 'practices'}
			<Practices />
		{:else if shell.view === 'workflows'}
			<Workflows />
		{:else if shell.view === 'review'}
			<Review />
		{:else if shell.view === 'mcp'}
			<Mcp />
		{:else if shell.view === 'settings'}
			<Settings />
		{/if}
	</div>

	<div class="foot" title={footer}>
		<Icon
			name={failed ? 'circle-x' : 'circle-check'}
			size={11}
			color={failed ? 'var(--danger-text)' : 'var(--success-text)'}
		/>
		<span class="mono line">{footer}</span>
	</div>
	<ResizeBar
		label="Resize side panel"
		value={shell.sidePanelWidth}
		min={SIDEPANEL_MIN}
		max={SIDEPANEL_MAX}
		gap={6}
		onlive={(w) => node?.style.setProperty('--side-w', `${w}px`)}
		onresize={setSidePanelWidth}
		testid="sidepanel-resize"
	/>
</aside>

<style>
	.side {
		position: relative;
		width: var(--side-w, 220px);
		flex: 0 0 var(--side-w, 220px);
	}

	.head {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 10px;
	}

	.title {
		font-weight: 600;
	}

	.spacer {
		flex: 1;
	}

	.body {
		flex: 1;
		display: flex;
		flex-direction: column;
		padding-bottom: 4px;
		overflow-y: auto;
	}

	.foot {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 6px 10px;
		border-top: 1px solid var(--border-subtle);
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.line {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
</style>
