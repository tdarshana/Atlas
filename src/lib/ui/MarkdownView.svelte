<script lang="ts">
	// The one shared Markdown viewer. Full use (the README card, the Frameworks preview pane)
	// shows a header with the document path and a Preview/Source switch; embedded use (the
	// DocEditor and PracticeDialog `Preview` toggle) hides that header and always renders
	// Preview, since the textarea next to it is already the source view. The mode is a single
	// preference shared by every viewer that shows the switch, remembered across restarts.
	import { untrack } from 'svelte';
	import { persistSet } from '$lib/shell/persist';
	import { inTauri } from '$lib/shell/platform';
	import { highlightSource, renderMarkdown } from './markdown';

	type Mode = 'preview' | 'source';

	interface Props {
		/** Raw Markdown source. Source mode always shows this, whole. */
		source: string;
		/** Rendered by Preview instead of `source`, for a document with a leading block
		 * that should not be rendered as prose (a `SKILL.md`'s frontmatter). Source mode
		 * still shows `source`, so nothing is hidden, only unrendered. */
		preview?: string;
		/** Shown in the header next to the switch, e.g. a document path. */
		path?: string;
		/** False hides the header and the switch, and always renders Preview. */
		showHeader?: boolean;
		/** Shown in place of the content when `source` is empty. */
		emptyText?: string;
	}

	let {
		source,
		preview,
		path,
		showHeader = true,
		emptyText = 'Nothing to preview yet.'
	}: Props = $props();

	const MODE_KEY = 'atlas.markdown.mode';

	function readStoredMode(): Mode {
		try {
			if (typeof localStorage === 'undefined') return 'preview';
			return localStorage.getItem(MODE_KEY) === 'source' ? 'source' : 'preview';
		} catch {
			return 'preview';
		}
	}

	function writeStoredMode(next: Mode): void {
		try {
			if (typeof localStorage !== 'undefined') localStorage.setItem(MODE_KEY, next);
		} catch {
			/* storage is unavailable; the mode is simply not remembered */
		}
		void persistSet(MODE_KEY, next);
	}

	// Read once at creation, the same way the DS table reads its persisted order: this is not
	// meant to track a later change to `showHeader`, which no call site changes after mount.
	let mode = $state<Mode>(untrack(() => (showHeader ? readStoredMode() : 'preview')));

	function setMode(next: Mode): void {
		mode = next;
		writeStoredMode(next);
	}

	const previewSource = $derived(preview ?? source);

	const contentHtml = $derived(
		mode === 'preview'
			? previewSource
				? renderMarkdown(previewSource)
				: ''
			: source
				? highlightSource(source)
				: ''
	);

	async function openLink(href: string): Promise<void> {
		if (inTauri()) {
			const { openUrl } = await import('@tauri-apps/plugin-opener');
			await openUrl(href);
		} else {
			window.open(href, '_blank', 'noopener,noreferrer');
		}
	}

	/** Every link the sanitised HTML can contain is a plain http(s) anchor (see `markdown.ts`),
	    so one delegated handler on the container covers all of them. */
	function onBodyClick(event: MouseEvent): void {
		const anchor = (event.target as HTMLElement).closest<HTMLAnchorElement>('a[href]');
		if (!anchor) return;
		event.preventDefault();
		void openLink(anchor.getAttribute('href')!);
	}
</script>

{#if showHeader}
	<header class="header">
		{#if path}<span class="path mono">{path}</span>{/if}
		<span class="spacer"></span>
		<div class="switch" role="tablist" aria-label="Markdown view mode">
			<button
				type="button"
				role="tab"
				aria-selected={mode === 'preview'}
				class:active={mode === 'preview'}
				data-testid="markdown-mode-preview"
				onclick={() => setMode('preview')}
			>
				Preview
			</button>
			<button
				type="button"
				role="tab"
				aria-selected={mode === 'source'}
				class:active={mode === 'source'}
				data-testid="markdown-mode-source"
				onclick={() => setMode('source')}
			>
				Source
			</button>
		</div>
	</header>
{/if}

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<!-- Every link in the sanitised content is a real, focusable `<a>`; this only intercepts the
     click it already dispatches on Enter, so no separate keyboard handling is needed here. -->
<div
	class="body"
	class:embedded={!showHeader}
	data-testid="markdown-body"
	onclick={onBodyClick}
>
	{#if !source}
		<p class="empty">{emptyText}</p>
	{:else}
		{@html contentHtml}
	{/if}
</div>

<style>
	.header {
		display: flex;
		align-items: center;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 var(--space-3);
		border-bottom: var(--border-width) solid var(--border-subtle);
		gap: var(--space-2);
	}

	.path {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--text-tertiary);
	}

	.spacer {
		flex: 1;
	}

	.switch {
		display: flex;
		flex: 0 0 auto;
		border: var(--border-width) solid var(--border-default);
		border-radius: var(--radius-sm);
		overflow: hidden;
	}

	.switch button {
		border: 0;
		background: transparent;
		color: var(--text-secondary);
		font: var(--type-ui);
		padding: 0 var(--space-2);
		height: var(--h-control-sm);
		cursor: pointer;
	}

	.switch button + button {
		border-left: var(--border-width) solid var(--border-default);
	}

	.switch button.active {
		background: var(--accent-muted);
		color: var(--text-primary);
	}

	.body {
		flex: 1;
		min-height: 0;
		min-width: 0;
		overflow: auto;
		padding: var(--space-2) var(--space-3);
		color: var(--text-secondary);
		font: var(--type-ui);
		/* A long inline code span (a path, a brace list of file names) has no break
		   opportunity of its own; without this it sets the panel's minimum width and
		   pushes the whole detail sideways. Fenced blocks keep their own scroll. */
		overflow-wrap: anywhere;
	}

	.body.embedded {
		flex: 0 0 auto;
		max-height: 240px;
		margin-top: var(--space-1);
		border: var(--border-width) solid var(--border-default);
		border-radius: var(--radius-sm);
		background: var(--bg-base);
	}

	.empty {
		margin: 0;
		color: var(--text-tertiary);
	}

	/* The rendered document. `{@html}` content carries no Svelte scope attribute, so every
	   selector below stays under `:global()`. */
	.body :global(h1),
	.body :global(h2),
	.body :global(h3),
	.body :global(h4),
	.body :global(h5),
	.body :global(h6) {
		margin: var(--space-3) 0 var(--space-1);
		color: var(--text-primary);
		font-family: var(--font-ui);
		font-weight: var(--weight-semibold);
	}

	.body :global(h1:first-child),
	.body :global(h2:first-child),
	.body :global(h3:first-child) {
		margin-top: 0;
	}

	.body :global(h1) {
		font-size: var(--text-lg);
		line-height: var(--text-lg-lh);
	}

	.body :global(h2) {
		font-size: var(--text-lg);
		line-height: var(--text-lg-lh);
		font-weight: var(--weight-medium);
	}

	.body :global(h3),
	.body :global(h4),
	.body :global(h5),
	.body :global(h6) {
		font-size: var(--text-base);
		line-height: var(--text-base-lh);
	}

	.body :global(p) {
		margin: var(--space-1) 0 var(--space-3);
	}

	.body :global(p:last-child) {
		margin-bottom: 0;
	}

	.body :global(ul),
	.body :global(ol) {
		margin: var(--space-1) 0 var(--space-3);
		padding-left: var(--space-5);
	}

	.body :global(li) {
		margin: var(--space-1) 0;
	}

	.body :global(a) {
		color: var(--accent);
	}

	.body :global(blockquote) {
		margin: var(--space-1) 0 var(--space-3);
		padding: 0 var(--space-3);
		border-left: 2px solid var(--border-strong);
		color: var(--text-tertiary);
	}

	.body :global(hr) {
		margin: var(--space-3) 0;
		border: 0;
		border-top: var(--border-width) solid var(--border-subtle);
	}

	.body :global(code) {
		font-family: var(--font-mono);
		font-size: var(--mono-sm);
	}

	.body :global(:not(pre) > code) {
		padding: 0 3px;
		border-radius: var(--radius-sm);
		background: var(--bg-inset);
	}

	.body :global(pre) {
		margin: var(--space-1) 0 var(--space-3);
		padding: var(--space-2) var(--space-3);
		border-radius: var(--radius-sm);
		background: var(--bg-inset);
		overflow: auto;
	}

	.body :global(pre code) {
		font-size: var(--mono-sm);
		line-height: var(--mono-sm-lh);
	}

	/* The Table look: a subtle-bordered header row and row dividers, no zebra striping. */
	.body :global(table) {
		margin: var(--space-1) 0 var(--space-3);
		width: 100%;
		border-collapse: collapse;
		font-size: var(--text-sm);
	}

	.body :global(th),
	.body :global(td) {
		padding: var(--space-1) var(--space-2);
		border-bottom: var(--border-width) solid var(--border-subtle);
		text-align: left;
	}

	.body :global(th) {
		color: var(--text-secondary);
		font-weight: var(--weight-medium);
		border-bottom: var(--border-width) solid var(--border-default);
	}

	/* A highlight theme derived from the DS colour tokens, not an imported hljs stylesheet;
	   the tokens already flip per theme, so this needs only one set of rules. */
	.body :global(.hljs-comment),
	.body :global(.hljs-quote) {
		color: var(--text-tertiary);
		font-style: italic;
	}

	.body :global(.hljs-keyword),
	.body :global(.hljs-selector-tag),
	.body :global(.hljs-literal),
	.body :global(.hljs-section),
	.body :global(.hljs-link) {
		color: var(--accent);
	}

	.body :global(.hljs-string),
	.body :global(.hljs-meta-string),
	.body :global(.hljs-addition),
	.body :global(.hljs-regexp) {
		color: var(--success-text);
	}

	.body :global(.hljs-number),
	.body :global(.hljs-symbol),
	.body :global(.hljs-bullet) {
		color: var(--info-text);
	}

	.body :global(.hljs-title),
	.body :global(.hljs-type),
	.body :global(.hljs-built_in),
	.body :global(.hljs-class .hljs-title) {
		color: var(--warning-text);
	}

	.body :global(.hljs-attr),
	.body :global(.hljs-attribute),
	.body :global(.hljs-variable),
	.body :global(.hljs-template-variable),
	.body :global(.hljs-name),
	.body :global(.hljs-tag) {
		color: var(--text-primary);
	}

	.body :global(.hljs-deletion) {
		color: var(--danger-text);
	}

	.body :global(.hljs-emphasis) {
		font-style: italic;
	}

	.body :global(.hljs-strong) {
		font-weight: var(--weight-semibold);
	}
</style>
