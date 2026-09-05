<script lang="ts">
	// The window shell: title bar, then the rail, the contextual side panel and the content
	// panel inset on the window background, then the status bar. The page never scrolls;
	// panels scroll inside themselves.
	import { onMount, type Snippet } from 'svelte';
	import { page } from '$app/state';
	import '../app.css';
	import { boot, daemon } from '$lib/daemon.svelte';
	import { refreshStatus, startStatusPolling, status } from '$lib/stores/status.svelte';
	import { desktop } from '$lib/shell/platform';
	import { connectChanges, onChange } from '$lib/stores/changes.svelte';
	import { loadSettings, settings } from '$lib/stores/settings.svelte';
	import { UI_THEME_KEY } from '$lib/types';
	import { Button } from '$lib/ds';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Toast from '$lib/ui/Toast.svelte';
	import CommandPalette from '$lib/search/CommandPalette.svelte';
	import ToolHost from '$lib/plugins/ToolHost.svelte';
	import ActivityRail from '$lib/shell/ActivityRail.svelte';
	import SidePanel from '$lib/shell/SidePanel.svelte';
	import StatusBar from '$lib/shell/StatusBar.svelte';
	import TitleBar from '$lib/shell/TitleBar.svelte';
	import { applyDaemonTheme, initShell, setView, shell } from '$lib/shell/shell.svelte';
	import { installGlobalShortcutBridge, installShortcuts, PALETTE_EVENT } from '$lib/shell/shortcuts';
	import { viewForPath, viewLabel } from '$lib/shell/views';
	import { hubTitle, project } from '$lib/stores/project.svelte';

	let { children }: { children: Snippet } = $props();

	onMount(() => {
		void initShell();
		void boot();
		const removeShortcuts = installShortcuts();
		let unlistenPalette: (() => void) | undefined;
		void installGlobalShortcutBridge().then((fn) => {
			unlistenPalette = fn;
		});
		return () => {
			removeShortcuts();
			unlistenPalette?.();
		};
	});

	// Polling starts once the daemon answers and stops if the connection is lost.
	$effect(() => {
		if (!daemon.ready) return;
		return startStatusPolling();
	});

	// The native About panel names the daemon's version and database once they are
	// known, and again only if they change (a daemon restart on a new build).
	let aboutSent = '';
	$effect(() => {
		const report = status.report;
		if (!report) return;
		const key = `${report.version}|${report.db_path}`;
		if (key === aboutSent) return;
		aboutSent = key;
		void desktop('about_menu_refresh', { daemonVersion: report.version, dbPath: report.db_path }, () => undefined);
	});

	// The change stream is what keeps every view live; polling is the fallback. The
	// status line follows the stream too, so the memory count moves as agents write.
	$effect(() => {
		if (!daemon.ready) return;
		connectChanges();
		return onChange('memory', () => void refreshStatus());
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

	/** The title bar's command box, handed to the palette as its anchor. */
	let commandRef = $state<HTMLElement | null>(null);

	// Inside the project hub the command box names the project and the open tab, so the
	// box says where you are rather than repeating the rail label eight times.
	const title = $derived(
		hubTitle(page.url.pathname, project.current) ?? viewLabel(shell.view)
	);
</script>

<TitleBar
	platform={shell.platform}
	{title}
	oncommand={openPalette}
	bind:commandRef
/>

<!-- Drops out of the title bar, lined up with the command box above. -->
<CommandPalette anchor={commandRef} />

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

<!-- Hidden frames for the plugins that contribute MCP tools, plus the daemon's channel
     for the calls it forwards. Renders nothing on screen and nothing at all outside
     Tauri; it waits for the daemon because the channel is the daemon's socket. -->
{#if daemon.ready}
	<ToolHost />
{/if}

<Toast />

<style>
	/* Pinned to the viewport: `overflow: hidden` alone still lets anchor navigation
	   (`/settings#about`) scroll the root and push the whole shell out of view. */
	:global(body) {
		position: fixed;
		inset: 0;
		display: flex;
		flex-direction: column;
		/* Under the UI scale setting's `zoom`, WebKit sizes `inset: 0` to the unzoomed
		   height and the bottom of the shell falls off screen. Dividing by the same factor
		   (`--ui-zoom`, set beside `zoom` by the boot script and `applyAppearance`) lands the
		   body exactly on the viewport, at 100% as much as at 150%. */
		height: calc(100vh / var(--ui-zoom, 1));
		overflow: hidden;
		background: var(--bg-base);
	}

	/* No top padding: the title bar already centres its command box, so any extra space
	   here would make the gap under the box larger than the gap above it. */
	.body {
		flex: 1;
		display: flex;
		min-height: 0;
		gap: 6px;
		padding: 0 6px 0 0;
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
