<script lang="ts">
	// The window shell: title bar, then the rail, the contextual side panel and the content
	// panel inset on the window background, then the status bar. The page never scrolls;
	// panels scroll inside themselves.
	import { onMount, type Snippet } from 'svelte';
	import { page } from '$app/state';
	import '../app.css';
	import { boot, daemon } from '$lib/daemon.svelte';
	import { startStatusPolling } from '$lib/stores/status.svelte';
	import { loadSettings, settings } from '$lib/stores/settings.svelte';
	import { UI_THEME_KEY } from '$lib/types';
	import Button from '$lib/ui/Button.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Toast from '$lib/ui/Toast.svelte';
	import CommandPalette from '$lib/search/CommandPalette.svelte';
	import ActivityRail from '$lib/shell/ActivityRail.svelte';
	import SidePanel from '$lib/shell/SidePanel.svelte';
	import StatusBar from '$lib/shell/StatusBar.svelte';
	import TitleBar from '$lib/shell/TitleBar.svelte';
	import { applyDaemonTheme, initShell, setView, shell } from '$lib/shell/shell.svelte';
	import { installShortcuts, PALETTE_EVENT } from '$lib/shell/shortcuts';
	import { viewForPath, viewLabel } from '$lib/shell/views';

	let { children }: { children: Snippet } = $props();

	onMount(() => {
		void initShell();
		void boot();
		return installShortcuts();
	});

	// Polling starts once the daemon answers and stops if the connection is lost.
	$effect(() => {
		if (!daemon.ready) return;
		return startStatusPolling();
	});

	// The daemon holds the shared theme, so read it once the daemon is up and reconcile.
	$effect(() => {
		if (daemon.ready && !settings.loaded) void loadSettings();
	});

	$effect(() => {
		if (settings.loaded) applyDaemonTheme(settings.values[UI_THEME_KEY]);
	});

	$effect(() => {
		setView(viewForPath(page.url.pathname));
	});

	function openPalette() {
		window.dispatchEvent(new CustomEvent(PALETTE_EVENT));
	}
</script>

<TitleBar platform={shell.platform} title={viewLabel(shell.view)} oncommand={openPalette} />

<!-- Drops out of the title bar over the command box; it measures the box itself. -->
<CommandPalette />

<div class="body">
	<ActivityRail />
	{#if shell.sidePanel && shell.view !== 'dashboard'}
		<SidePanel />
	{/if}
	<main class="panel content">
		{#if daemon.error}
			<div class="fill" data-testid="daemon-error">
				<ErrorState message={daemon.error} logPath={daemon.logPath}>
					<Button variant="primary" onclick={() => boot()}>Retry</Button>
				</ErrorState>
			</div>
		{:else if !daemon.ready}
			<div class="fill"><p class="booting">Starting Atlas…</p></div>
		{:else}
			{@render children()}
		{/if}
	</main>
</div>

<StatusBar />

<Toast />

<style>
	:global(body) {
		display: flex;
		flex-direction: column;
		height: 100vh;
		overflow: hidden;
		background: var(--bg-base);
	}

	.body {
		flex: 1;
		display: flex;
		min-height: 0;
		gap: 6px;
		padding: 6px 6px 6px 0;
	}

	.content {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		padding: 16px;
		gap: 12px;
	}

	.fill {
		display: flex;
		align-items: center;
		justify-content: center;
		height: 100%;
	}

	.booting {
		margin: 0;
		color: var(--text-secondary);
	}
</style>
