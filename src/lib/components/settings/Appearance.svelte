<script lang="ts">
	// Appearance: theme (Dark, Light, a bundled preset, a plugin's theme or an imported
	// pack), fonts, the base UI size and the UI scale. Nothing here touches the document,
	// localStorage or the daemon until Save; the live preview strip below reflects the
	// draft immediately, scoped to itself via `data-theme` and inline custom properties
	// rather than the real window.
	import { Button, Select } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { inTauri, type Theme } from '$lib/shell';
	import { applyAppearance, SCALE_OPTIONS, type UiScale } from '$lib/shell/appearance';
	import {
		FONT_MONO_OPTIONS,
		FONT_SIZE_OPTIONS,
		FONT_UI_OPTIONS,
		fontMonoStack,
		fontUiStack,
		type FontMono,
		type FontSize,
		type FontUi
	} from '$lib/shell/fonts';
	import { validateThemePack } from '$lib/shell/theme-pack';
	import { THEME_PRESETS, themePreset } from '$lib/shell/theme-presets';
	import { contributions } from '$lib/plugins/host.svelte';
	import { loadPluginThemes } from '$lib/plugins/themes';
	import { loadSettings, settingNumber, settingString } from '$lib/stores/settings.svelte';
	import {
		UI_FONT_MONO_KEY,
		UI_FONT_SIZE_KEY,
		UI_FONT_UI_KEY,
		UI_SCALE_KEY,
		UI_THEME_PACK_KEY
	} from '$lib/types';
	import type { Settings, ThemePack } from '$lib/types';
	import { push } from '$lib/platform/toasts.svelte';
	import SettingsCard from './SettingsCard.svelte';

	let draftTheme = $state<Theme>('dark');
	let draftPack = $state<ThemePack | null>(null);
	let draftFontUi = $state<FontUi>('system');
	let draftFontMono = $state<FontMono>('jetbrains-mono');
	let draftFontSize = $state<FontSize>(12);
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
		const fontUi = settingString(UI_FONT_UI_KEY);
		draftFontUi = fontUi === 'inter' || fontUi === 'jetbrains-mono' ? fontUi : 'system';
		draftFontMono = settingString(UI_FONT_MONO_KEY) === 'system-mono' ? 'system-mono' : 'jetbrains-mono';
		const size = settingNumber(UI_FONT_SIZE_KEY, 12);
		draftFontSize = size === 11 || size === 13 ? size : 12;
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
				[UI_SCALE_KEY]: draftScale
			};
			await api().setSettings(partial);
			await loadSettings();
			applyAppearance(base, draftPack, draftFontUi, draftFontMono, draftFontSize, draftScale);
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
		/>
		<Select
			label="UI size"
			options={FONT_SIZE_OPTIONS}
			value={String(draftFontSize)}
			data-testid="appearance-font-size"
			onchange={(e) => {
				draftFontSize = Number(e.currentTarget.value) as FontSize;
			}}
		/>
	</div>
	<div class="pair">
		<Select
			label="UI scale"
			options={SCALE_OPTIONS}
			value={String(draftScale)}
			data-testid="appearance-scale"
			onchange={(e) => {
				draftScale = Number(e.currentTarget.value) as UiScale;
			}}
		/>
	</div>
	<div class="pair">
		<Select
			label="UI family"
			options={FONT_UI_OPTIONS}
			value={draftFontUi}
			data-testid="appearance-font-ui"
			onchange={(e) => {
				draftFontUi = e.currentTarget.value as FontUi;
			}}
		/>
		<Select
			label="Mono family"
			options={FONT_MONO_OPTIONS}
			value={draftFontMono}
			data-testid="appearance-font-mono"
			onchange={(e) => {
				draftFontMono = e.currentTarget.value as FontMono;
			}}
		/>
	</div>

	<div class="preview" data-testid="appearance-preview" data-theme={previewBase} style={previewTokenStyle}>
		<span class="preview-ui" style={`font-family:${fontUiStack(draftFontUi)};font-size:${draftFontSize}px`}>
			Atlas keeps every agent's memory in one place.
		</span>
		<span class="preview-mono" style={`font-family:${fontMonoStack(draftFontMono)}`}>
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
</style>
