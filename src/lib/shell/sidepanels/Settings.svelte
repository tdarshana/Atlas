<script lang="ts">
	// The settings sections, and where the daemon keeps its files. Each row is a link to
	// the matching card's anchor on the settings page, which scrolls itself to the hash.
	import type { IconName } from '$lib/ds';
	import { scrollToSection, SETTINGS_SECTIONS } from '$lib/components/settings-sections';
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

	/** One row per card, in the page's order, so a card added to the shared list shows
	 * up here without a second list to keep in step. */
	const ICONS: Record<string, IconName> = {
		daemon: 'cpu',
		cli: 'terminal',
		extraction: 'wand-sparkles',
		'board-stages': 'columns-3',
		appearance: 'palette',
		mcp: 'plug',
		plugins: 'layers',
		permissions: 'shield-check',
		skills: 'graduation-cap',
		shortcuts: 'keyboard',
		notifications: 'bell',
		security: 'lock',
		about: 'info'
	};

	/** The daemon reports the database file; its directory is the config home. */
	const home = $derived.by(() => {
		const path = status.report?.db_path ?? '';
		const cut = path.lastIndexOf('/');
		return cut > 0 ? path.slice(0, cut) : '~/.atlas';
	});
</script>

<TreeGroup label="Sections">
	{#each SETTINGS_SECTIONS as section (section.id)}
		<TreeRow
			icon={ICONS[section.id] ?? 'settings'}
			label={section.label}
			href="/settings#{section.id}"
			onclick={jump(`#${section.id}`)}
		/>
	{/each}
</TreeGroup>

<span class="spacer"></span>

<TreeGroup label="Config" count={home} initialOpen={false} />

<style>
	.spacer {
		flex: 1;
	}
</style>
