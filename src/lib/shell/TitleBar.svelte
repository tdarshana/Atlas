<script lang="ts">
	// The window's top strip: OS chrome on the outside, back/forward and the command box in
	// the middle. It shares the window background with the rail and carries no bottom
	// border, so the inset panels below read as floating on the window.
	import type { Snippet } from 'svelte';
	import { Icon, type Platform } from '$lib/ds';

	interface Props {
		platform: Platform;
		/** The view name shown inside the command box. */
		title: string;
		/** Task 5 swaps this for the live palette input. */
		commandBox?: Snippet;
		right?: Snippet;
		oncommand?: () => void;
	}

	let { platform, title, commandBox, right, oncommand }: Props = $props();

	const MENUS = ['File', 'Edit', 'View', 'Window', 'Help'];

	function back() {
		history.back();
	}

	function forward() {
		history.forward();
	}

	async function control(action: 'minimize' | 'toggleMaximize' | 'close') {
		try {
			const { getCurrentWindow } = await import('@tauri-apps/api/window');
			await getCurrentWindow()[action]();
		} catch {
			/* outside Tauri there is no window to drive */
		}
	}
</script>

<div class="dbm-titlebar" data-tauri-drag-region role="banner">
	{#if platform === 'mac'}
		<!-- The window uses the overlay title bar, so macOS draws the traffic lights here. -->
		<span class="lights" aria-hidden="true"></span>
	{:else}
		<span class="dbm-titlebar__menu">
			{#each MENUS as menu (menu)}
				<button type="button" tabindex="-1">{menu}</button>
			{/each}
		</span>
	{/if}

	<span class="dbm-titlebar__spacer"></span>

	<div class="centre">
		<button class="nav" type="button" aria-label="Back" title="Back" onclick={back}>
			<Icon name="arrow-left" size={13} color="var(--text-tertiary)" />
		</button>
		<button class="nav" type="button" aria-label="Forward" title="Forward" onclick={forward}>
			<Icon name="arrow-right" size={13} color="var(--text-tertiary)" />
		</button>
		{#if commandBox}
			{@render commandBox()}
		{:else}
			<button class="command" type="button" data-testid="titlebar-command" onclick={oncommand}>
				<Icon name="search" size={12} />
				<span>Atlas · {title}</span>
			</button>
		{/if}
	</div>

	<span class="dbm-titlebar__spacer"></span>

	{@render right?.()}

	{#if platform !== 'mac'}
		<span class="dbm-titlebar__wincontrols">
			<button type="button" aria-label="Minimise" onclick={() => control('minimize')}>
				<Icon name="minus" size={13} />
			</button>
			<button type="button" aria-label="Maximise" onclick={() => control('toggleMaximize')}>
				<Icon name="square" size={11} />
			</button>
			<button type="button" aria-label="Close" onclick={() => control('close')}>
				<Icon name="x" size={13} />
			</button>
		</span>
	{/if}
</div>

<style>
	/* The design file makes the same two overrides: no seam under the bar, and the window
	   background rather than the DS raised surface. */
	.dbm-titlebar {
		border-bottom: 0;
		background: var(--bg-base);
	}

	/* Room for the traffic lights macOS paints over the webview. */
	.lights {
		width: 78px;
		flex: 0 0 78px;
	}

	.centre {
		display: flex;
		align-items: center;
		gap: 10px;
		-webkit-app-region: no-drag;
	}

	.nav {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 18px;
		height: 18px;
		padding: 0;
		border: 0;
		border-radius: var(--radius-sm);
		background: transparent;
		color: var(--text-tertiary);
		cursor: default;
		transition: var(--transition-hover);
	}

	.nav:hover {
		background: var(--bg-hover);
		color: var(--text-primary);
	}

	.command {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 640px;
		height: 24px;
		padding: 0 8px;
		background: var(--bg-base);
		border: 1px solid var(--border-default);
		border-radius: 3px;
		color: var(--text-tertiary);
		font-family: inherit;
		font-size: 12px;
		text-align: left;
		cursor: default;
		transition: var(--transition-hover);
	}

	.command:hover {
		border-color: var(--border-strong);
	}
</style>
