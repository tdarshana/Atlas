<script lang="ts">
	// The settings sections, and where the daemon keeps its files. Each row is a link to
	// the matching card's anchor on the settings page, which scrolls itself to the hash.
	import { scrollToSection } from '$lib/components/settings-sections';
	import { status } from '$lib/stores/status.svelte';
	import TreeGroup from '../TreeGroup.svelte';
	import TreeRow from '../TreeRow.svelte';

	/**
	 * The href alone only moves the page the first time: picking the same row again leaves
	 * the URL where it is, so the settings page never hears about it. Scroll on the click
	 * too, which is a no-op when the card is not on screen yet and the page's own effect
	 * takes over.
	 */
	const jump = (hash: string) => () =>
		scrollToSection(hash, true, (id) => document.getElementById(id));

	/** The daemon reports the database file; its directory is the config home. */
	const home = $derived.by(() => {
		const path = status.report?.db_path ?? '';
		const cut = path.lastIndexOf('/');
		return cut > 0 ? path.slice(0, cut) : '~/.atlas';
	});
</script>

<TreeGroup label="Sections">
	<TreeRow icon="cpu" label="Daemon" href="/settings#daemon" onclick={jump('#daemon')} />
	<TreeRow icon="wand-sparkles" label="Extraction" href="/settings#extraction" onclick={jump('#extraction')} />
	<TreeRow icon="columns-3" label="Board stages" href="/settings#board-stages" onclick={jump('#board-stages')} />
	<TreeRow icon="palette" label="Appearance" href="/settings#appearance" onclick={jump('#appearance')} />
	<TreeRow icon="plug" label="MCP server" href="/settings#mcp" onclick={jump('#mcp')} />
	<TreeRow icon="layers" label="Plugins" href="/settings#plugins" onclick={jump('#plugins')} />
	<TreeRow icon="keyboard" label="Shortcuts" href="/settings#shortcuts" onclick={jump('#shortcuts')} />
	<TreeRow icon="bell" label="Notifications" href="/settings#notifications" onclick={jump('#notifications')} />
	<TreeRow icon="lock" label="Security" href="/settings#security" onclick={jump('#security')} />
	<TreeRow icon="info" label="About" href="/settings#about" onclick={jump('#about')} />
</TreeGroup>

<span class="spacer"></span>

<TreeGroup label="Config" count={home} initialOpen={false} />

<style>
	.spacer {
		flex: 1;
	}
</style>
