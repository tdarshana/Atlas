<script lang="ts">
	// Appearance: theme (Dark, Light, a bundled preset, a plugin's theme or an imported
	// pack), fonts, the base UI size and the UI scale. Nothing here touches the document,
	// localStorage or the daemon until Save; the live preview strip below reflects the
	// draft immediately, scoped to itself via `data-theme` and inline custom properties
	// rather than the real window.
	import { onMount } from 'svelte';
	import { Button, Icon, IconButton, Select, Switch } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { inTauri, type Theme } from '$lib/shell';
	import { applyAppearance, SCALE_OPTIONS, setSmoothing, type UiScale } from '$lib/shell/appearance';
	import {
		DEFAULT_FONT_MONO,
		DEFAULT_FONT_SIZE,
		DEFAULT_FONT_UI,
		DEFAULT_MONO_SIZE,
		FONT_MONO_OPTIONS,
		FONT_SIZE_OPTIONS,
		FONT_SIZES,
		FONT_UI_OPTIONS,
		fontMonoStack,
		fontUiStack,
		isMonospace,
		loadInstalledFonts,
		MONO_SIZE_OPTIONS,
		MONO_SIZES,
		type FontMono,
		type FontSize,
		type FontUi,
		type InstalledFont
	} from '$lib/shell/fonts';
	import MenuSelect from '$lib/components/board/MenuSelect.svelte';
	import { validateThemePack } from '$lib/shell/theme-pack';
	import { THEME_PRESETS, themePreset } from '$lib/shell/theme-presets';
	import { contributions } from '$lib/plugins/host.svelte';
	import { loadPluginThemes } from '$lib/plugins/themes';
	import { loadSettings, saveSettings, settingBool, settingNumber, settingString } from '$lib/stores/settings.svelte';
	import {
		UI_FONT_MONO_KEY,
		UI_FONT_MONO_SIZE_KEY,
		UI_FONT_SIZE_KEY,
		UI_FONT_SMOOTHING_KEY,
		UI_FONT_UI_KEY,
		UI_SCALE_KEY,
		UI_THEME_PACK_KEY
	} from '$lib/types';
	import type { Settings, ThemePack } from '$lib/types';
	import { push } from '$lib/platform/toasts.svelte';
	import SettingsCard from './SettingsCard.svelte';

	let draftTheme = $state<Theme>('dark');
	let draftPack = $state<ThemePack | null>(null);
	let draftFontUi = $state<FontUi>(DEFAULT_FONT_UI);
	let draftFontMono = $state<FontMono>(DEFAULT_FONT_MONO);
	let draftFontSize = $state<FontSize>(12);
	let draftMonoSize = $state<FontSize>(12);
	let draftSmoothing = $state(true);

	/** Each Typography row shows a reset arrow only while it differs from the default. */
	const uiFontChanged = $derived(draftFontUi !== DEFAULT_FONT_UI || draftFontSize !== DEFAULT_FONT_SIZE);
	const monoFontChanged = $derived(draftFontMono !== DEFAULT_FONT_MONO || draftMonoSize !== DEFAULT_MONO_SIZE);

	/** Smoothing applies the moment it is flipped, unlike the rest of the card, which
	 * waits for Save: the change is easiest to judge on the text around the switch. */
	function onSmoothingChange(on: boolean): void {
		draftSmoothing = on;
		setSmoothing(on);
		saveSettings({ [UI_FONT_SMOOTHING_KEY]: on }).catch((e) => push('error', errorMessage(e)));
	}

	/** The machine's font families, from the host; empty in a plain browser. */
	let installedFonts = $state<InstalledFont[]>([]);
	onMount(() => {
		void loadInstalledFonts().then((fonts) => (installedFonts = fonts));
	});

	/** Presets first, then every installed proportional family drawn in its own face
	 * (monospace ones belong to the code picker). A stored family the machine no longer
	 * has still appears, so the trigger never shows a blank. */
	const uiFontOptions = $derived.by(() => {
		const rows = [
			...FONT_UI_OPTIONS,
			...installedFonts
				.filter((f) => !isMonospace(f))
				.map((f) => ({ value: f.family, label: f.family, font: fontUiStack(f.family) }))
		];
		if (!rows.some((r) => r.value === draftFontUi)) rows.push({ value: draftFontUi, label: draftFontUi });
		return rows;
	});
	/** Same for code, with only the monospace families. */
	const monoFontOptions = $derived.by(() => {
		const rows = [
			...FONT_MONO_OPTIONS,
			...installedFonts
				.filter(isMonospace)
				.map((f) => ({ value: f.family, label: f.family, font: fontMonoStack(f.family) }))
		];
		if (!rows.some((r) => r.value === draftFontMono)) rows.push({ value: draftFontMono, label: draftFontMono });
		return rows;
	});
	let draftScale = $state<UiScale>(100);

	let appearanceSaving = $state(false);
	let appearanceError = $state<string | null>(null);
	let importingTheme = $state(false);
	let importError = $state<string | null>(null);
	let fileInput = $state<HTMLInputElement | null>(null);

	/** The themes enabled plugins contribute, each already named `<name> (<plugin name>)`
	 * and validated the same way an imported pack is. */
	let pluginThemes = $state<ThemePack[]>([]);

	function pluginTheme(name: string): ThemePack | undefined {
		return pluginThemes.find((p) => p.name === name);
	}

	/** The base themes, the bundled presets, the contributed themes, and, when one is
	 * imported, its own entry: only one pack can be stored at a time, so there is never
	 * more than one extra option here. */
	const themeOptions = $derived([
		{ value: 'dark', label: 'Dark' },
		{ value: 'light', label: 'Light' },
		...THEME_PRESETS.map((p) => ({ value: p.name, label: p.name })),
		...pluginThemes.map((p) => ({ value: p.name, label: p.name })),
		...(draftPack && !themePreset(draftPack.name) && !pluginTheme(draftPack.name)
			? [{ value: draftPack.name, label: draftPack.name }]
			: [])
	]);
	const selectedTheme = $derived(draftPack ? draftPack.name : draftTheme);
	/** What the preview strip (and, on Save, the real window) paints: a pack's own base
	 * wins, since its tokens are built against that ramp. */
	const previewBase = $derived(draftPack ? draftPack.base : draftTheme);
	const previewTokenStyle = $derived(
		draftPack ? Object.entries(draftPack.tokens).map(([k, v]) => `${k}:${v}`).join(';') : ''
	);

	/** Picking Dark or Light drops the pack; picking a bundled preset or a plugin's theme
	 * makes it the draft pack; picking an imported pack's own name (the only other
	 * option) is a no-op. */
	function onThemeChange(value: string): void {
		if (value === 'dark' || value === 'light') {
			draftTheme = value;
			draftPack = null;
			return;
		}
		const pack = themePreset(value) ?? pluginTheme(value);
		if (pack) {
			draftTheme = pack.base;
			draftPack = { ...pack, tokens: { ...pack.tokens } };
		}
	}

	/** A pack read back from the daemon was already validated when it was saved; a
	 * failure here means the setting predates a token rename, so it is dropped rather
	 * than shown broken. */
	function parseStoredPack(raw: string): ThemePack | null {
		try {
			return validateThemePack(JSON.parse(raw));
		} catch {
			return null;
		}
	}

	/** Copies the saved appearance into the draft, discarding unsaved picks. The route
	 * calls it once the settings have loaded and again on Reload. */
	export function refresh(): void {
		draftTheme = settingString('ui.theme') === 'light' ? 'light' : 'dark';
		const packRaw = settingString(UI_THEME_PACK_KEY);
		draftPack = packRaw ? parseStoredPack(packRaw) : null;
		draftFontUi = settingString(UI_FONT_UI_KEY) || DEFAULT_FONT_UI;
		draftFontMono = settingString(UI_FONT_MONO_KEY) || DEFAULT_FONT_MONO;
		const size = settingNumber(UI_FONT_SIZE_KEY, 12);
		draftFontSize = FONT_SIZES.includes(size) ? size : 12;
		const monoSize = settingNumber(UI_FONT_MONO_SIZE_KEY, 12);
		draftMonoSize = MONO_SIZES.includes(monoSize) ? monoSize : 12;
		draftSmoothing = settingBool(UI_FONT_SMOOTHING_KEY, true);
		const scale = settingNumber(UI_SCALE_KEY, 100);
		draftScale = scale === 80 || scale === 90 || scale === 110 || scale === 125 || scale === 150 ? scale : 100;
	}

	/** In Tauri, the native open dialog scoped to the same Downloads/Documents/Desktop
	 * folders the export flow writes into; in the browser, a hidden file input. Answers
	 * null when the user cancelled. */
	async function readThemeFile(): Promise<string | null> {
		if (inTauri()) {
			const { open } = await import('@tauri-apps/plugin-dialog');
			const { downloadDir } = await import('@tauri-apps/api/path');
			const dir = await downloadDir().catch(() => undefined);
			const path = await open({ defaultPath: dir, filters: [{ name: 'Theme pack', extensions: ['json'] }] });
			if (!path || Array.isArray(path)) return null;
			const { readTextFile } = await import('@tauri-apps/plugin-fs');
			return readTextFile(path);
		}
		if (!fileInput) return null;
		fileInput.value = '';
		return new Promise((resolve, reject) => {
			if (!fileInput) return resolve(null);
			fileInput.onchange = () => {
				const file = fileInput?.files?.[0];
				if (!file) return resolve(null);
				file.text().then(resolve, reject);
			};
			fileInput.click();
		});
	}

	async function importTheme(): Promise<void> {
		importError = null;
		importingTheme = true;
		try {
			const text = await readThemeFile();
			if (text === null) return;
			const pack = validateThemePack(JSON.parse(text));
			draftPack = pack;
			draftTheme = pack.base;
		} catch (e) {
			importError = errorMessage(e);
		} finally {
			importingTheme = false;
		}
	}

	async function saveAppearance(): Promise<void> {
		appearanceSaving = true;
		appearanceError = null;
		try {
			const base: Theme = draftPack ? draftPack.base : draftTheme;
			const partial: Settings = {
				'ui.theme': base,
				[UI_THEME_PACK_KEY]: draftPack ? JSON.stringify(draftPack) : null,
				[UI_FONT_UI_KEY]: draftFontUi,
				[UI_FONT_MONO_KEY]: draftFontMono,
				[UI_FONT_SIZE_KEY]: draftFontSize,
				[UI_FONT_MONO_SIZE_KEY]: draftMonoSize,
				[UI_FONT_SMOOTHING_KEY]: draftSmoothing,
				[UI_SCALE_KEY]: draftScale
			};
			await api().setSettings(partial);
			await loadSettings();
			applyAppearance(base, draftPack, draftFontUi, draftFontMono, draftFontSize, draftScale, draftMonoSize, draftSmoothing);
			push('success', 'Appearance saved');
		} catch (e) {
			appearanceError = errorMessage(e);
			push('error', appearanceError);
		} finally {
			appearanceSaving = false;
		}
	}

	// The contributed themes follow the plugin list: enabling a plugin from the Plugins
	// page and coming back here shows its themes without a reload.
	//
	// Each run reads every theme file, so two runs started close together (enable then
	// disable a plugin quickly) can land out of order. The generation counter drops any
	// answer that is not the newest, rather than letting a slow earlier read overwrite a
	// fast later one and leave the select stale until the next change.
	let themeLoadGeneration = 0;
	$effect(() => {
		const contribs = contributions();
		const generation = ++themeLoadGeneration;
		void loadPluginThemes(contribs).then((packs) => {
			if (generation === themeLoadGeneration) pluginThemes = packs;
		});
	});
</script>

<SettingsCard id="appearance" title="Appearance">
	{#snippet head()}
		<span class="spacer"></span>
		<Button
			variant="ghost"
			size="sm"
			data-testid="appearance-import"
			disabled={importingTheme}
			onclick={importTheme}
		>
			{importingTheme ? 'Importing…' : 'Import theme…'}
		</Button>
		<Button
			variant="primary"
			size="sm"
			data-testid="appearance-save"
			disabled={appearanceSaving}
			onclick={saveAppearance}
		>
			{appearanceSaving ? 'Saving…' : 'Save'}
		</Button>
	{/snippet}

	{#if importError}
		<p class="bad" role="alert" data-testid="appearance-import-error">{importError}</p>
	{/if}
	{#if appearanceError}
		<p class="bad" role="alert" data-testid="appearance-error">{appearanceError}</p>
	{/if}

	<div class="pair">
		<Select
			label="Theme"
			options={themeOptions}
			value={selectedTheme}
			data-testid="appearance-theme"
			onchange={(e) => onThemeChange(e.currentTarget.value)}
			onreset={selectedTheme !== 'dark' ? () => onThemeChange('dark') : undefined}
		/>
		<Select
			label="UI scale"
			options={SCALE_OPTIONS}
			value={String(draftScale)}
			data-testid="appearance-scale"
			onchange={(e) => {
				draftScale = Number(e.currentTarget.value) as UiScale;
			}}
			onreset={draftScale !== 100 ? () => (draftScale = 100) : undefined}
		/>
	</div>

	<div class="preview" data-testid="appearance-preview" data-theme={previewBase} style={previewTokenStyle}>
		<span class="preview-ui" style={`font-family:${fontUiStack(draftFontUi)};font-size:${draftFontSize}px`}>
			Atlas keeps every agent's memory in one place.
		</span>
		<span class="preview-mono" style={`font-family:${fontMonoStack(draftFontMono)};font-size:${draftMonoSize}px`}>
			atlas recall "duckdb schema"
		</span>
		<div class="swatches">
			<span class="swatch" style="background:var(--bg-base)" title="--bg-base"></span>
			<span class="swatch" style="background:var(--bg-surface)" title="--bg-surface"></span>
			<span class="swatch" style="background:var(--accent)" title="--accent"></span>
			<span class="swatch" style="background:var(--text-primary)" title="--text-primary"></span>
		</div>
	</div>

	<span class="hint">
		A theme pack overrides colour tokens and the two radius tokens on top of its base
		theme. Import a JSON file shaped
		<span class="mono">{'{ "name", "base": "dark"|"light", "tokens": { "--token": "value" } }'}</span>.
	</span>

	<h4 class="subheading">Typography</h4>

	<div class="typo-row">
		<div class="typo-text">
			<span class="typo-label">
				Interface font
				{#if uiFontChanged}
					<IconButton
						size="sm"
						icon="undo-2"
						label="Reset interface font"
						data-testid="appearance-font-ui-reset"
						onclick={() => {
							draftFontUi = DEFAULT_FONT_UI;
							draftFontSize = DEFAULT_FONT_SIZE;
						}}
					/>
				{/if}
			</span>
			<span class="typo-hint">Everything outside code blocks and the terminal.</span>
		</div>
		<div class="typo-controls">
			<MenuSelect
				value={draftFontUi}
				options={uiFontOptions}
				searchable
				placeholder="System"
				testId="appearance-font-ui"
				onchange={(v) => (draftFontUi = v)}
			/>
			<Select
				options={FONT_SIZE_OPTIONS}
				value={String(draftFontSize)}
				aria-label="Interface font size"
				data-testid="appearance-font-size"
				onchange={(e) => (draftFontSize = Number(e.currentTarget.value))}
			/>
		</div>
	</div>
	<div
		class="typo-preview"
		data-testid="appearance-ui-preview"
		style={`font-family:${fontUiStack(draftFontUi)};font-size:${draftFontSize}px`}
	>
		Ask <span class="chip chip-persona"><Icon name="user-round" size={12} /> Reviewer</span> to recall
		<span class="chip"><span class="chip-mono">duckdb schema</span></span> and move
		<span class="chip"><span class="chip-mono">ATL-42</span></span> to Testing before shipping.
	</div>

	<div class="typo-row">
		<div class="typo-text">
			<span class="typo-label">
				Monospace font
				{#if monoFontChanged}
					<IconButton
						size="sm"
						icon="undo-2"
						label="Reset monospace font"
						data-testid="appearance-font-mono-reset"
						onclick={() => {
							draftFontMono = DEFAULT_FONT_MONO;
							draftMonoSize = DEFAULT_MONO_SIZE;
						}}
					/>
				{/if}
			</span>
			<span class="typo-hint">Code blocks, diffs, file previews and the terminal.</span>
		</div>
		<div class="typo-controls">
			<MenuSelect
				value={draftFontMono}
				options={monoFontOptions}
				searchable
				placeholder="JetBrains Mono"
				testId="appearance-font-mono"
				onchange={(v) => (draftFontMono = v)}
			/>
			<Select
				options={MONO_SIZE_OPTIONS}
				value={String(draftMonoSize)}
				aria-label="Monospace font size"
				data-testid="appearance-mono-size"
				onchange={(e) => (draftMonoSize = Number(e.currentTarget.value))}
			/>
		</div>
	</div>
	<div
		class="typo-preview code"
		data-testid="appearance-mono-preview"
		style={`font-family:${fontMonoStack(draftFontMono)};font-size:${draftMonoSize}px`}
	>
		<div class="code-head">
			<span class="code-file">crates/atlas-core/src/board/mod.rs</span>
			<span class="code-stat"><span class="del">-1</span> <span class="add">+1</span></span>
		</div>
		<div class="code-line"><span class="ln">1</span><span>pub fn ready(task: &Task) -&gt; bool {'{'}</span></div>
		<div class="code-line removed"><span class="ln">2</span><span>    task.blocked_by.is_empty()</span></div>
		<div class="code-line added"><span class="ln">2</span><span>    task.open_blockers == 0 // 0O 1lI</span></div>
		<div class="code-line"><span class="ln">3</span><span>{'}'}</span></div>
	</div>

	<div class="typo-row toggle">
		<div class="typo-text">
			<span class="typo-label">
				Font smoothing
				{#if !draftSmoothing}
					<IconButton
						size="sm"
						icon="undo-2"
						label="Reset font smoothing"
						data-testid="appearance-smoothing-reset"
						onclick={() => onSmoothingChange(true)}
					/>
				{/if}
			</span>
			<span class="typo-hint">Render text with thinner grayscale anti-aliasing instead of macOS's heavier default. Applies as you flip it.</span>
		</div>
		<Switch
			checked={draftSmoothing}
			aria-label="Font smoothing"
			data-testid="appearance-smoothing"
			onchange={(e) => onSmoothingChange(e.currentTarget.checked)}
		/>
	</div>

	<input
		bind:this={fileInput}
		type="file"
		accept="application/json"
		data-testid="appearance-import-input"
		style="display:none"
	/>
</SettingsCard>

<style>
	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
	}

	.spacer {
		flex: 1;
	}

	.mono {
		font-family: var(--font-mono);
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}

	/* Scoped to itself: `data-theme` plus a pack's own inline custom properties, so the
	   swatches (plain `var(--token)` backgrounds) preview the draft without touching the
	   real window. */
	.preview {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 12px;
		border: 1px solid var(--border-default);
		border-radius: var(--radius-md);
		background: var(--bg-base);
		color: var(--text-primary);
	}

	.preview-mono {
		color: var(--text-secondary);
	}

	.swatches {
		display: flex;
		gap: 6px;
	}

	.swatch {
		width: 20px;
		height: 20px;
		border-radius: var(--radius-sm);
		border: 1px solid var(--border-subtle);
	}

	.subheading {
		margin: 8px 0 0;
		font-size: 13px;
		font-weight: var(--weight-semibold);
	}

	.typo-row {
		display: grid;
		grid-template-columns: 1fr minmax(280px, 42%);
		gap: 12px;
		align-items: start;
	}

	.typo-row.toggle {
		grid-template-columns: 1fr auto;
		align-items: center;
	}

	.typo-text {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.typo-label {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		min-height: 20px;
		font-size: var(--text-sm);
		font-weight: var(--weight-medium);
	}

	.typo-hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.typo-controls {
		display: grid;
		grid-template-columns: 1fr 96px;
		gap: 8px;
		align-items: end;
	}

	/* The previews wear the draft font and size inline; everything else is the app's. */
	.typo-preview {
		padding: 10px 12px;
		border: 1px solid var(--border-default);
		border-radius: var(--radius-md);
		background: var(--bg-raised);
		color: var(--text-primary);
		line-height: 1.6;
	}

	.chip {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		padding: 0 6px;
		border: 1px solid var(--border-default);
		border-radius: 999px;
		background: var(--bg-surface);
		vertical-align: baseline;
	}

	.chip-persona {
		color: var(--accent);
		border-color: var(--accent-muted);
		background: var(--accent-muted);
	}

	.chip-mono {
		font-family: var(--font-mono);
		font-size: 0.92em;
	}

	.typo-preview.code {
		padding: 8px 0;
		line-height: 1.7;
	}

	.code-head {
		display: flex;
		justify-content: space-between;
		padding: 0 12px 6px;
	}

	.code-file {
		color: var(--text-secondary);
	}

	.del {
		color: var(--danger-text);
	}

	.add {
		color: var(--success-text);
	}

	.code-line {
		display: flex;
		gap: 12px;
		padding: 0 12px;
		white-space: pre;
	}

	.ln {
		flex: none;
		width: 2ch;
		text-align: right;
		color: var(--text-tertiary);
	}

	.removed {
		background: var(--danger-muted);
	}

	.added {
		background: var(--success-muted);
	}
</style>
