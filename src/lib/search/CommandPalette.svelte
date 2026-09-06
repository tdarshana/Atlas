<script lang="ts">
	// The palette drops out of the title bar with no scrim: its input sits exactly where
	// the command box is, at the same 640px width, and grows to 32px as it takes focus.
	// Three states share one list: the empty state (recent, jump to, commands), the
	// project picker a trailing `@` opens, and the grouped results.
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { Icon, KeyHint, type IconName } from '$lib/ds';
	import { plural } from '$lib/format';
	import { contributions, loadPlugins, plugins } from '$lib/plugins/host.svelte';
	import { shell } from '$lib/shell/shell.svelte';
	import { PALETTE_EVENT } from '$lib/shell/shortcuts';
	import { VIEWS, type ViewDef } from '$lib/shell/views';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import type { Project, SearchHit, SearchKind } from '$lib/types';
	import { filterCommands, pluginCommands, type PaletteCommand } from './commands';
	import { highlightSegments, TYPE_PREFIXES } from './parse';
	import {
		chooseScope,
		choosePrefix,
		clearScopes,
		closePalette,
		openItem,
		openPalette,
		palette,
		parsed,
		popScope,
		removeScope,
		scopeChips,
		setInput,
		type OpenMode
	} from './palette.svelte';

	interface Props {
		/**
		 * The title bar's command box. The palette lines its input up with this element
		 * and hands focus back to it on close. The layout binds the element and passes it
		 * here; without one the palette still opens, at the window's left edge.
		 */
		anchor?: HTMLElement | null;
	}

	let { anchor = null }: Props = $props();

	const WIDTH = 640;
	const LIST_ID = 'palette-list';
	const rowId = (i: number) => `palette-row-${i}`;

	let root: HTMLDivElement | undefined = $state();
	let box: HTMLInputElement | undefined = $state();
	let left = $state(0);

	// ---- rows ----

	type Row =
		| { kind: 'hit'; hit: SearchHit }
		| { kind: 'command'; command: PaletteCommand }
		| { kind: 'jump'; view: ViewDef }
		| { kind: 'project'; project: Project }
		| { kind: 'all-projects' }
		| { kind: 'prefix'; prefix: string; label: string; hint: string; icon: IconName };

	interface Group {
		label: string;
		meta: string;
		rows: Row[];
	}

	/** A group with its rows numbered into the flat list the selection indexes. */
	interface LaidOut {
		label: string;
		meta: string;
		rows: { row: Row; index: number }[];
	}

	const KIND_ICON: Record<SearchKind, IconName> = {
		task: 'columns-3',
		memory: 'braces',
		project: 'folder',
		file: 'file',
		commit: 'git-commit-horizontal',
		event: 'history',
		workflow: 'git-branch'
	};

	const KIND_LABEL: Record<SearchKind, string> = {
		task: 'Tasks',
		memory: 'Memories',
		project: 'Projects',
		file: 'Files',
		commit: 'Commits',
		event: 'Log',
		workflow: 'Workflows'
	};

	const PREFIX_ROWS: Record<string, { label: string; hint: string; icon: IconName }> = {
		'#': { label: 'Tasks only', hint: 'e.g. # duckdb', icon: 'columns-3' },
		'~': { label: 'Memories only', hint: 'e.g. ~ decision', icon: 'database' },
		'>': { label: 'Commands', hint: 'e.g. > sync', icon: 'terminal' }
	};

	const query = $derived(parsed());
	/** The contributed commands, listed beside the app's own. Nothing renders until the
	 * store has been read once, and outside Tauri there is nothing to read. */
	const extraCommands = $derived(plugins.loaded ? pluginCommands(contributions()) : []);
	const chips = $derived(scopeChips());
	const scopeNames = $derived(chips.map((c) => c.name).join(' '));

	const currentProjectId = $derived.by(() => {
		const m = /^\/projects\/([^/]+)/.exec(page.url.pathname);
		return m ? m[1] : null;
	});

	const pickerProjects = $derived.by(() => {
		const needle = query.text.trim().toLowerCase();
		if (!needle) return projects.items;
		return projects.items.filter(
			(p) =>
				p.name.toLowerCase().includes(needle) || p.root_path.toLowerCase().includes(needle)
		);
	});

	const groups = $derived.by((): Group[] => {
		if (query.type === 'project-picker') {
			return [
				{
					label: 'Scope to project',
					meta: 'type to filter · space to add another',
					rows: [
						...pickerProjects.map((project): Row => ({ kind: 'project', project })),
						{ kind: 'all-projects' } as Row
					]
				},
				{
					label: 'Other scopes',
					meta: '',
					rows: TYPE_PREFIXES.map(
						(p): Row => ({ kind: 'prefix', prefix: p.prefix, ...PREFIX_ROWS[p.prefix] })
					)
				}
			];
		}

		if (query.type === 'command') {
			return [
				{
					label: 'Commands',
					meta: '',
					rows: filterCommands(query.text, extraCommands).map((command): Row => ({ kind: 'command', command }))
				}
			];
		}

		if (!query.text.trim()) {
			const out: Group[] = [];
			if (palette.recent.length) {
				out.push({
					label: 'Recent',
					meta: 'recently opened',
					rows: palette.recent.map((hit): Row => ({ kind: 'hit', hit }))
				});
			}
			out.push({
				label: 'Jump to',
				meta: '',
				rows: VIEWS.map((view): Row => ({ kind: 'jump', view }))
			});
			out.push({
				label: 'Commands',
				meta: '',
				rows: filterCommands('', extraCommands).map((command): Row => ({ kind: 'command', command }))
			});
			return out;
		}

		const result = palette.results;
		if (!result) return [];
		return result.groups
			.filter((g) => g.items.length > 0)
			.map((g) => ({
				label: KIND_LABEL[g.kind],
				meta: scopeNames ? `${g.items.length} in ${scopeNames}` : String(g.items.length),
				rows: g.items.map((hit): Row => ({ kind: 'hit', hit }))
			}));
	});

	const flat = $derived(groups.flatMap((g) => g.rows));

	const laidOut = $derived.by((): LaidOut[] => {
		let n = 0;
		return groups.map((g) => ({
			label: g.label,
			meta: g.meta,
			rows: g.rows.map((row) => ({ row, index: n++ }))
		}));
	});

	// Rows change under the selection whenever the query does; keep it in range.
	$effect(() => {
		if (palette.selected >= flat.length) palette.selected = Math.max(0, flat.length - 1);
	});

	// The list scrolls inside a 520px panel, so a selection moved past the fold has to
	// be brought back or the highlight disappears and Enter becomes a guess.
	$effect(() => {
		const i = palette.selected;
		if (!palette.open || !flat.length) return;
		const el = root?.querySelector(`#${rowId(i)}`);
		// jsdom and older webviews have no `scrollIntoView`; the highlight is still drawn.
		if (el && typeof el.scrollIntoView === 'function') el.scrollIntoView({ block: 'nearest' });
	});

	// Keeps the box in step when the store rewrites the text under it, which one-way
	// binding misses whenever the stripped remainder happens to equal the old value.
	$effect(() => {
		const value = palette.input;
		if (box && box.value !== value) box.value = value;
	});

	// ---- opening and closing ----

	function measure() {
		if (!anchor) return;
		left = anchor.getBoundingClientRect().left;
	}

	async function open() {
		measure();
		openPalette();
		void loadProjects();
		if (!plugins.loaded && plugins.available) void loadPlugins();
		await tick();
		box?.focus();
	}

	function close() {
		closePalette();
		anchor?.focus();
	}

	/** ⌘K or a click on the command box while the palette is open means "put it away". */
	function toggle() {
		if (palette.open) close();
		else void open();
	}

	/** With no scrim there is nothing to click through, so the window reports it. */
	function onpointerdown(e: PointerEvent) {
		if (!palette.open) return;
		const target = e.target;
		if (!(target instanceof Node)) return;
		// A click on the command box itself toggles the palette; leave it to that handler.
		if (root?.contains(target) || anchor?.contains(target)) return;
		close();
	}

	$effect(() => {
		window.addEventListener(PALETTE_EVENT, toggle);
		window.addEventListener('resize', measure);
		window.addEventListener('pointerdown', onpointerdown);
		return () => {
			window.removeEventListener(PALETTE_EVENT, toggle);
			window.removeEventListener('resize', measure);
			window.removeEventListener('pointerdown', onpointerdown);
		};
	});

	function activate(row: Row, mode: OpenMode = 'open') {
		switch (row.kind) {
			case 'hit':
				void openItem(row.hit, mode);
				break;
			case 'command': {
				const command = row.command;
				close();
				void command.run({ projectId: currentProjectId, goto: (href) => goto(href) });
				break;
			}
			case 'jump':
				close();
				void goto(row.view.href);
				break;
			case 'project':
				chooseScope(row.project.name);
				box?.focus();
				break;
			case 'all-projects':
				clearScopes();
				setInput('');
				box?.focus();
				break;
			case 'prefix':
				choosePrefix(row.prefix);
				box?.focus();
				break;
		}
	}

	function move(delta: number) {
		if (!flat.length) return;
		palette.selected = Math.min(flat.length - 1, Math.max(0, palette.selected + delta));
	}

	function modeFor(e: KeyboardEvent): OpenMode {
		if (e.metaKey || e.ctrlKey) return 'inspector';
		if (e.altKey) return 'board';
		return 'open';
	}

	/**
	 * Bound to the panel, not the input, so Escape still works when focus has moved to
	 * a chip's remove button or a row. A focused button keeps its own Enter and
	 * Backspace; the list keys are the palette's everywhere else.
	 */
	function onkeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			e.preventDefault();
			close();
			return;
		}
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			move(1);
			return;
		}
		if (e.key === 'ArrowUp') {
			e.preventDefault();
			move(-1);
			return;
		}

		const onButton = e.target instanceof HTMLElement && e.target.tagName === 'BUTTON';
		if (onButton) return;

		if (e.key === 'Enter') {
			e.preventDefault();
			const row = flat[palette.selected];
			if (row) activate(row, modeFor(e));
			return;
		}
		if (e.key === 'Backspace' && palette.input === '') {
			e.preventDefault();
			popScope();
		}
	}

</script>

{#if palette.open}
	<div
		bind:this={root}
		class="palette"
		style="left:{left}px;width:{WIDTH}px"
		role="dialog"
		aria-label="Search Atlas"
		tabindex="-1"
		data-testid="command-palette"
		{onkeydown}
	>
		<div class="box">
			<Icon name="search" size={13} color="var(--text-tertiary)" />
			{#each chips as chip (chip.name)}
				<span class="chip" class:chip--unknown={chip.id === null}>
					<Icon
						name={chip.id === null ? 'alert-triangle' : 'folder'}
						size={11}
						color={chip.id === null ? 'var(--warning-text)' : 'var(--accent)'}
					/>
					{chip.name}
					<button
						class="chip-x"
						type="button"
						aria-label="Remove scope {chip.name}"
						onclick={() => removeScope(chip.name)}
					>
						<Icon name="x" size={10} color="var(--text-tertiary)" />
					</button>
				</span>
			{/each}
			<input
				autocorrect="off"
				autocapitalize="off"
				bind:this={box}
				class="field"
				type="text"
				autocomplete="off"
				spellcheck="false"
				role="combobox"
				aria-label="Search Atlas"
				aria-expanded="true"
				aria-controls={LIST_ID}
				aria-autocomplete="list"
				aria-activedescendant={flat.length ? rowId(palette.selected) : undefined}
				placeholder="Search tasks, memories, projects, files, commands…"
				value={palette.input}
				oninput={(e) => setInput(e.currentTarget.value)}
			/>
			<KeyHint combo="Escape" platform={shell.platform} />
		</div>

		<div class="list" id={LIST_ID} role="listbox" tabindex="-1" aria-label="Search results">
			{#each laidOut as group (group.label)}
				<div class="head">
					<span>{group.label}</span>
					<span class="spacer"></span>
					<span class="head-meta">{group.meta}</span>
				</div>
				{#each group.rows as { row, index: i } (i)}
					<button
						class="row"
						class:row--on={i === palette.selected}
						id={rowId(i)}
						type="button"
						role="option"
						aria-selected={i === palette.selected}
						onmouseenter={() => (palette.selected = i)}
						onclick={() => activate(row)}
					>
						{#if row.kind === 'hit'}
							<Icon name={KIND_ICON[row.hit.kind]} size={13} color="var(--text-tertiary)" />
							<span class="label">
								{#if row.hit.reference && row.hit.kind === 'task'}<span class="mono"
										>{row.hit.reference}</span
									> · {/if}{#each highlightSegments(row.hit.title, row.hit.highlights) as seg, si (si)}{#if seg.mark}<mark
										>{seg.text}</mark
									>{:else}{seg.text}{/if}{/each}{#if row.hit.subtitle}<span class="meta"
										>{row.hit.subtitle}</span
									>{/if}
							</span>
							<span class="badge">{row.hit.kind}</span>
						{:else if row.kind === 'command'}
							<Icon name={row.command.icon} size={13} color="var(--text-tertiary)" />
							<span class="label"
								>{row.command.label}{#if row.command.hint}<span class="meta"
										>{row.command.hint}</span
									>{/if}</span
							>
							{#if row.command.combo}
								<KeyHint combo={row.command.combo} platform={shell.platform} />
							{/if}
							<span class="badge">command</span>
						{:else if row.kind === 'jump'}
							<Icon name={row.view.icon} size={13} color="var(--text-tertiary)" />
							<span class="label">{row.view.label}</span>
							<KeyHint combo={row.view.combo} platform={shell.platform} />
						{:else if row.kind === 'project'}
							<Icon name="folder" size={13} color="var(--accent)" />
							<span class="label"
								><span class="mono">{row.project.name}</span><span class="meta"
									>{row.project.root_path}</span
								></span
							>
							<span class="badge">project</span>
						{:else if row.kind === 'all-projects'}
							<Icon name="layers" size={13} color="var(--text-tertiary)" />
							<span class="label">All projects<span class="meta">default · no scope</span></span>
						{:else}
							<Icon name={row.icon} size={13} color="var(--text-tertiary)" />
							<span class="label"
								><span class="mono">{row.prefix}</span>
								{row.label}<span class="meta">{row.hint}</span></span
							>
						{/if}
					</button>
				{/each}
			{/each}

			{#if !groups.length}
				<p class="empty">
					{#if palette.loading}Searching…{:else if palette.error}{palette.error}{:else}No matches.{/if}
				</p>
			{/if}
		</div>

		<div class="foot">
			{#if query.type === 'project-picker'}
				<span class="hint"><KeyHint combo="Enter" platform={shell.platform} /> select scope</span>
				<span class="hint"
					><KeyHint combo="Backspace" platform={shell.platform} /> remove scope</span
				>
				<span class="spacer"></span>
				<span>Scopes stack: <span class="mono">@atlas @Habitmaker</span> searches both</span>
			{:else}
				<span class="hint">
					<KeyHint combo="Up" platform={shell.platform} />
					<KeyHint combo="Down" platform={shell.platform} /> navigate
				</span>
				<span class="hint"><KeyHint combo="Enter" platform={shell.platform} /> open</span>
				<span class="hint"
					><KeyHint combo="Mod+Enter" platform={shell.platform} /> open in inspector</span
				>
				<span class="hint"
					><KeyHint combo="Alt+Enter" platform={shell.platform} /> open board filtered</span
				>
				<span class="spacer"></span>
				{#if palette.results}
					<span class="mono">{plural(palette.total, 'result')} · {palette.took_ms} ms</span>
				{:else if !query.text.trim()}
					<!-- Frame 01.1: the prefixes only announce themselves on the empty box. -->
					<span class="legend">
						<KeyHint combo="@" platform={shell.platform} /> project
						<KeyHint combo="#" platform={shell.platform} /> task
						<KeyHint combo="~" platform={shell.platform} /> memory
						<KeyHint combo=">" platform={shell.platform} /> command
					</span>
				{/if}
			{/if}
		</div>
	</div>
{/if}

<style>
	.palette {
		position: fixed;
		top: 3px;
		z-index: 80;
		display: flex;
		flex-direction: column;
		max-height: 520px;
		background: var(--bg-overlay);
		border: 1px solid var(--border-default);
		border-radius: 3px;
		box-shadow: var(--shadow-lg);
		overflow: hidden;
		font-size: 12px;
		animation: drop 120ms var(--ease-overlay);
	}

	@keyframes drop {
		from {
			opacity: 0;
			transform: translateY(-4px);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.palette {
			animation-duration: 0ms;
		}
	}

	.box {
		display: flex;
		align-items: center;
		gap: 8px;
		flex: 0 0 32px;
		height: 32px;
		margin: 4px;
		padding: 0 10px;
		background: var(--bg-base);
		border: 1px solid var(--accent);
		border-radius: 3px;
		font-size: 13px;
	}

	.field {
		flex: 1;
		min-width: 0;
		border: 0;
		background: transparent;
		color: var(--text-primary);
		font: inherit;
		outline: none;
	}

	.field::placeholder {
		color: var(--text-tertiary);
	}

	.chip {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		flex: 0 0 auto;
		height: 18px;
		padding: 0 6px;
		background: var(--accent-muted);
		border: 1px solid var(--accent);
		border-radius: 2px;
		color: var(--text-primary);
		font-family: var(--font-mono);
		font-size: 11px;
	}

	.chip--unknown {
		background: var(--warning-muted);
		border-color: var(--warning);
	}

	.chip-x {
		display: inline-flex;
		align-items: center;
		padding: 0;
		border: 0;
		background: transparent;
		cursor: default;
	}

	.list {
		flex: 1;
		min-height: 0;
		overflow: auto;
		padding: 4px 0;
	}

	.head {
		display: flex;
		align-items: center;
		padding: 6px 12px 2px;
		color: var(--text-tertiary);
		font-size: 11px;
		font-weight: 500;
		text-transform: uppercase;
		letter-spacing: 0.04em;
	}

	.head-meta {
		text-transform: none;
		letter-spacing: 0;
	}

	.spacer {
		flex: 1;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		height: 26px;
		padding: 0 12px;
		border: 0;
		background: transparent;
		color: var(--text-primary);
		font: inherit;
		text-align: left;
		cursor: default;
	}

	.row--on {
		background: var(--accent-muted);
		box-shadow: inset 2px 0 0 var(--accent);
	}

	.label {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.label mark {
		background: transparent;
		color: var(--accent);
		font-weight: 600;
	}

	.meta {
		margin-left: 8px;
		color: var(--text-tertiary);
		font-family: var(--font-mono);
		font-size: 11px;
	}

	.mono {
		font-family: var(--font-mono);
	}

	.badge {
		flex: 0 0 auto;
		height: 15px;
		padding: 0 4px;
		border: 1px solid var(--border-default);
		border-radius: 2px;
		color: var(--text-tertiary);
		font-size: 11px;
		line-height: 13px;
		text-transform: uppercase;
		letter-spacing: 0.03em;
	}

	.empty {
		margin: 0;
		padding: 8px 12px;
		color: var(--text-tertiary);
	}

	.foot {
		display: flex;
		align-items: center;
		gap: 12px;
		flex: 0 0 24px;
		height: 24px;
		padding: 0 12px;
		border-top: 1px solid var(--border-subtle);
		background: var(--bg-raised);
		color: var(--text-tertiary);
		font-size: 11px;
	}

	.hint {
		display: flex;
		align-items: center;
		gap: 4px;
	}

	.legend {
		display: flex;
		align-items: center;
		gap: 6px;
	}
</style>
