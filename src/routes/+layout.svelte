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

<Toast />

<style>
	/* Pinned to the viewport: `overflow: hidden` alone still lets anchor navigation
	   (`/settings#about`) scroll the root and push the whole shell out of view. */
	:global(body) {
		position: fixed;
		inset: 0;
		display: flex;
		flex-direction: column;
		height: 100vh;
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
		padding: 0 6px 6px 0;
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
