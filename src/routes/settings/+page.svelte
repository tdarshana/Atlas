<script lang="ts">
	// Settings (frame 10): the seven keys the daemon stores, the global board stages, and
	// how agents reach the MCP server. Port and embedding model are read only because the
	// daemon sets them at startup. Save sends only the keys that changed, and the API key
	// only when one was typed: the server hands back "***" for a stored key and reads a
	// missing key as "leave it alone".
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { Badge, Button, Checkbox, Icon, Input, KeyHint, Select, Table, type TableColumn } from '$lib/ds';
	import { api, daemon } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { NOTHING, relativeAge } from '$lib/format';
	import { copyText, inTauri, setStatusItems, type Theme } from '$lib/shell';
	import { applyAppearance } from '$lib/shell/appearance';
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
	import { desktop } from '$lib/shell/platform';
	import { validateThemePack } from '$lib/shell/theme-pack';
	import { mirrorKey, vaultHintText, vaultStatus } from '$lib/shell/vault';
	import { VIEWS } from '$lib/shell/views';
	import { openLogFolder } from '$lib/search/commands';
	import { CLAUDE_SNIPPET, CODEX_SNIPPET, nextDisabledTools, toolIcon } from '$lib/mcp';
	import { scrollToSection } from '$lib/components/settings-sections';
	import { diagnosticsText } from '$lib/components/settings/diagnostics';
	import { updateProgressPercent } from '$lib/components/settings/update-progress';
	import {
		acceleratorToKeyHintCombo,
		blurRecording,
		cancelRecording,
		captureKey,
		comboToAccelerator,
		INITIAL_RECORDER_STATE,
		startRecording as startRecordingCombo,
		type RecorderState
	} from '$lib/components/settings/shortcut-recorder';
	import StageEditor from '$lib/components/StageEditor.svelte';
	import {
		DEFAULT_MIN_CONFIDENCE,
		MASKED,
		changedSettings,
		draftFromSettings,
		loadSettings,
		saveSettings,
		settingNumber,
		settingString,
		settings
	} from '$lib/stores/settings.svelte';
	import { status } from '$lib/stores/status.svelte';
	import { UI_FONT_MONO_KEY, UI_FONT_SIZE_KEY, UI_FONT_UI_KEY, UI_THEME_PACK_KEY } from '$lib/types';
	import type {
		AboutInfo,
		ExtractionTestResult,
		McpClient,
		McpStatusReport,
		McpToolRow,
		Settings,
		Stage,
		ThemePack,
		UpdateCheckResult,
		UpdateProgress,
		VaultStatus
	} from '$lib/types';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	/** Draft values. The API key starts blank on every load: blank means unchanged. */
	let enabled = $state(false);
	let baseUrl = $state('');
	let apiKey = $state('');
	let model = $state('');
	let threshold = $state(DEFAULT_MIN_CONFIDENCE);

	let saving = $state(false);
	let saveError = $state<string | null>(null);

	let testing = $state(false);
	let testResult = $state<ExtractionTestResult | null>(null);

	/** The global stage list, which every project without an override follows. */
	let stages = $state<Stage[]>([]);
	let stagesError = $state<string | null>(null);

	let copied = $state<string | null>(null);

	/** The button needs extraction on and both endpoint fields filled to mean anything. */
	const testDisabled = $derived(!enabled || baseUrl.trim() === '' || model.trim() === '' || testing);

	/** True once the daemon holds a key, which is all `"***"` tells us. */
	const keyStored = $derived(settings.values['extraction.api_key'] === MASKED);

	/**
	 * A key is entered against one endpoint, so the daemon drops it when the base URL
	 * moves without a new one: nothing local can point Atlas somewhere else and have the
	 * old key follow it there. Warn before saving, not after, since the key has to be
	 * typed again either way. A trailing slash is not a move; neither is it to the daemon.
	 */
	const trimTrailingSlash = (url: string) => url.trim().replace(/\/+$/, '');
	const keyWillBeCleared = $derived(
		keyStored &&
			apiKey === '' &&
			trimTrailingSlash(baseUrl) !== trimTrailingSlash(settingString('extraction.base_url'))
	);

	const port = $derived(settingString('daemon.port', String(daemon.port)));
	const embeddingModel = $derived(settingString('embedding.model', 'not set') || 'not set');
	const online = $derived(!daemon.error);

	// -- Start Atlas at login --------------------------------------------------------
	//
	// The checkbox reflects the actual launch-agent state (`autostart_get`), not the
	// mirrored `ui.autostart` setting: another client's read of that mirror is a
	// convenience, not the source of truth this OS-level toggle has to agree with.
	let autostart = $state(false);
	let autostartBusy = $state(false);

	async function loadAutostart(): Promise<void> {
		autostart = await desktop('autostart_get', undefined, () => false);
	}

	async function toggleAutostart(next: boolean): Promise<void> {
		autostartBusy = true;
		try {
			await desktop('autostart_set', { enabled: next }, () => undefined);
			autostart = next;
			await api().setSettings({ 'ui.autostart': next });
		} catch (e) {
			push('error', errorMessage(e));
			await loadAutostart();
		} finally {
			autostartBusy = false;
		}
	}

	// -- Global shortcut recorder ------------------------------------------------------

	let recorder = $state<RecorderState>(INITIAL_RECORDER_STATE);
	let shortcutError = $state<string | null>(null);
	let applyingShortcut = $state(false);

	const capturedAccelerator = $derived(comboToAccelerator(recorder.combo));
	const storedShortcut = $derived(settingString('ui.global_shortcut'));
	/** What the recorder shows: the combo being captured, or the stored shortcut when
	 * nothing is being recorded right now. */
	const recorderCombo = $derived(
		capturedAccelerator
			? acceleratorToKeyHintCombo(capturedAccelerator)
			: storedShortcut
				? acceleratorToKeyHintCombo(storedShortcut)
				: ''
	);

	function startRecording(): void {
		recorder = startRecordingCombo();
		shortcutError = null;
	}

	/**
	 * Losing focus must not lose the captured combo: the Apply button sits right next
	 * to the recorder, so a plain click on it fires blur before click in a
	 * click-focuses-buttons engine, and clearing the combo here would make Apply
	 * unclickable on every attempt. `blurRecording` only stops recording.
	 */
	function onRecorderBlur(): void {
		recorder = blurRecording(recorder);
	}

	function onRecorderKeydown(e: KeyboardEvent): void {
		if (!recorder.recording) return;
		e.preventDefault();
		if (e.key === 'Escape') {
			recorder = cancelRecording();
			return;
		}
		recorder = captureKey(recorder, e);
	}

	async function applyShortcut(): Promise<void> {
		const accelerator = capturedAccelerator;
		if (!accelerator) return;
		applyingShortcut = true;
		shortcutError = null;
		try {
			await desktop('shortcut_set', { accelerator }, () => undefined);
			await api().setSettings({ 'ui.global_shortcut': accelerator });
			recorder = cancelRecording();
			push('success', 'Shortcut applied');
		} catch (e) {
			shortcutError = errorMessage(e);
		} finally {
			applyingShortcut = false;
		}
	}

	/** Mod+K, Mod+J and Mod+B are bound by the shell directly, not per-view; the rest
	 * come straight from the rail's own view list so this can never list a combo the
	 * shell does not actually bind. */
	const inAppShortcuts = [
		{ combo: 'Mod+K', label: 'Command palette' },
		{ combo: 'Mod+J', label: 'Toggle side panel' },
		{ combo: 'Mod+B', label: 'Toggle rail' },
		...VIEWS.map((v) => ({ combo: v.combo, label: v.label }))
	];

	// -- Notifications ------------------------------------------------------------------

	const notifyFlag = (key: string) => settings.values[key] === true;

	async function setNotifyFlag(key: string, value: boolean): Promise<void> {
		try {
			await api().setSettings({ [key]: value });
			await loadSettings();
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	/** `"granted"`, `"denied"`, `"default"` (not yet decided), or null before the first
	 * load; only `"default"` gets an "Allow notifications" button, since the other two
	 * are already decided one way or the other. */
	let notifyPermission = $state<string | null>(null);

	async function loadNotifyPermission(): Promise<void> {
		notifyPermission = await desktop('notification_permission', undefined, () => null);
	}

	async function requestNotifyPermission(): Promise<void> {
		notifyPermission = await desktop('notification_request_permission', undefined, () => notifyPermission);
	}

	const notifyPermissionHint = $derived(
		notifyPermission === 'granted'
			? 'Notifications are allowed.'
			: notifyPermission === 'denied'
				? 'Notifications are blocked in system settings.'
				: notifyPermission === 'default'
					? 'Notifications need permission before any of the above can show one.'
					: ''
	);

	let sendingTestNotification = $state(false);

	async function sendTestNotification(): Promise<void> {
		sendingTestNotification = true;
		try {
			await desktop('notify', { title: 'Atlas', body: 'This is a test notification.' }, async () => {
				if (typeof Notification === 'undefined') return;
				let permission = Notification.permission;
				if (permission === 'default') permission = await Notification.requestPermission();
				if (permission === 'granted') new Notification('Atlas', { body: 'This is a test notification.' });
			});
			push('success', 'Test notification sent');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			sendingTestNotification = false;
		}
	}

	// -- About ----------------------------------------------------------------------------

	let about = $state<AboutInfo | null>(null);
	let aboutError = $state<string | null>(null);

	async function loadAbout(): Promise<void> {
		if (!inTauri()) {
			about = null;
			aboutError = null;
			return;
		}
		try {
			about = await desktop('about_info', undefined, () => null);
		} catch (e) {
			aboutError = errorMessage(e);
		}
	}

	/** The persisted UI state (`ui_state_all`), folded into Copy diagnostics: a bug
	 * report often hinges on which rail, theme or filters were active. */
	let uiState = $state<Record<string, unknown>>({});

	async function loadUiState(): Promise<void> {
		uiState = await desktop('ui_state_all', undefined, () => ({}));
	}

	const diagnostics = $derived(
		about
			? diagnosticsText(
					{
						...about,
						daemon_version: status.report?.version ?? 'unknown',
						db_path: status.report?.db_path ?? 'unknown'
					},
					uiState
				)
			: ''
	);

	// -- Security (vault) --------------------------------------------------------------

	let vault = $state<VaultStatus>('missing');
	let vaultPassphrase = $state('');
	let vaultBusy = $state(false);
	let vaultError = $state<string | null>(null);
	let vaultScopes = $state<string[]>([]);

	async function loadVault(): Promise<void> {
		vault = await vaultStatus();
		vaultScopes = vault === 'unlocked' ? await desktop('vault_list', undefined, () => []) : [];
	}

	async function setVaultPassphrase(): Promise<void> {
		vaultBusy = true;
		vaultError = null;
		try {
			await desktop('vault_set_passphrase', { passphrase: vaultPassphrase }, () => undefined);
			vaultPassphrase = '';
			await loadVault();
			push('success', 'Vault passphrase set');
		} catch (e) {
			vaultError = errorMessage(e);
		} finally {
			vaultBusy = false;
		}
	}

	async function unlockVault(): Promise<void> {
		vaultBusy = true;
		vaultError = null;
		try {
			await desktop('vault_unlock', { passphrase: vaultPassphrase }, () => undefined);
			vaultPassphrase = '';
			await loadVault();
			push('success', 'Vault unlocked');
		} catch (e) {
			vaultError = errorMessage(e);
		} finally {
			vaultBusy = false;
		}
	}

	async function lockVault(): Promise<void> {
		vaultBusy = true;
		vaultError = null;
		try {
			await desktop('vault_lock', undefined, () => undefined);
			await loadVault();
		} catch (e) {
			vaultError = errorMessage(e);
		} finally {
			vaultBusy = false;
		}
	}

	async function reapplyVaultKey(scope: string): Promise<void> {
		vaultBusy = true;
		vaultError = null;
		try {
			await desktop('vault_reapply', { scope }, () => undefined);
			push('success', `Reapplied the key for ${scope}`);
		} catch (e) {
			vaultError = errorMessage(e);
		} finally {
			vaultBusy = false;
		}
	}

	// -- Updater --------------------------------------------------------------------

	let updateChecking = $state(false);
	let updateResult = $state<UpdateCheckResult | null>(null);
	let updateError = $state<string | null>(null);
	let updateInstalling = $state(false);
	let updateProgress = $state<UpdateProgress | null>(null);

	const updateProgressPct = $derived(updateProgressPercent(updateProgress));

	async function checkForUpdates(): Promise<void> {
		updateChecking = true;
		updateError = null;
		updateResult = null;
		try {
			updateResult = await desktop('update_check', undefined, () => {
				throw new Error('Only available in the desktop app.');
			});
		} catch (e) {
			updateError = errorMessage(e);
		} finally {
			updateChecking = false;
		}
	}

	async function installUpdate(): Promise<void> {
		if (!inTauri()) return;
		updateInstalling = true;
		updateError = null;
		updateProgress = { downloaded: 0, total: null };
		let unlisten: (() => void) | null = null;
		try {
			const { listen } = await import('@tauri-apps/api/event');
			unlisten = await listen<UpdateProgress>('atlas:update-progress', (e) => {
				updateProgress = e.payload;
			});
			const { invoke } = await import('@tauri-apps/api/core');
			// Resolves only on failure: a successful install relaunches the app before
			// this promise would otherwise settle.
			await invoke('update_install');
		} catch (e) {
			updateError = errorMessage(e);
		} finally {
			updateInstalling = false;
			unlisten?.();
		}
	}

	async function restartToUpdate(): Promise<void> {
		await desktop('app_relaunch', undefined, () => undefined);
	}

	/** The CONNECT block shows both snippets, one after the other, as frame 10 does. */
	const connectSnippet = `${CLAUDE_SNIPPET}\n\n${CODEX_SNIPPET}`;

	/** `GET /api/v1/mcp/status`: transports, counts, tools, resources, prompts, clients. */
	let mcp = $state<McpStatusReport | null>(null);
	let mcpError = $state<string | null>(null);
	let restarting = $state(false);
	let togglingTool = $state<string | null>(null);

	const mcpCountsText = $derived(
		mcp ? `${mcp.counts.tools} tools · ${mcp.counts.resources} resources · ${mcp.counts.prompts} prompts` : '…'
	);

	const toolColumns: TableColumn<McpToolRow>[] = [
		{ key: 'name', label: 'Name', width: '220px', mono: true, sortable: true },
		{ key: 'description', label: 'Description' },
		{ key: 'args', label: 'Arguments', width: '260px', mono: true },
		{ key: 'scope', label: 'Scope', width: '80px', sortable: true },
		{ key: 'enabled', label: 'Enable', width: '70px' }
	];

	const clientColumns: TableColumn<McpClient>[] = [
		{ key: 'client_name', label: 'Client', mono: true, sortable: true },
		{ key: 'transport', label: 'Transport', width: '90px', sortable: true },
		{ key: 'last_seen', label: 'Last seen', width: '110px', sortable: true },
		{ key: 'tool_calls', label: 'Calls', width: '70px', align: 'right', mono: true, sortable: true }
	];

	async function loadMcp(): Promise<void> {
		try {
			mcp = await api().mcpStatus();
			mcpError = null;
		} catch (e) {
			mcpError = errorMessage(e);
		}
	}

	/** Flips one tool's checkbox: writes `mcp.disabled_tools` with that name added or
	 * removed, leaving every other disabled tool untouched, then reloads the status. */
	async function toggleTool(row: McpToolRow): Promise<void> {
		if (!mcp) return;
		const currentlyDisabled = mcp.tools.filter((t) => !t.enabled).map((t) => t.name);
		const next = nextDisabledTools(currentlyDisabled, row.name, !row.enabled);
		togglingTool = row.name;
		try {
			await api().setSettings({ 'mcp.disabled_tools': next });
			await loadMcp();
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			togglingTool = null;
		}
	}

	/** Stop and start the daemon through the Tauri `daemon_restart` command. Only
	 * available inside the desktop app; the browser has no way to manage the process. */
	async function restartDaemon(): Promise<void> {
		if (!inTauri()) return;
		restarting = true;
		try {
			const { invoke } = await import('@tauri-apps/api/core');
			await invoke('daemon_restart');
			push('success', 'MCP server restarted');
			await loadMcp();
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			restarting = false;
		}
	}

	/** How long the button says "Copied" before going back to its own name. */
	const COPIED_MS = 1500;
	let copiedTimer: ReturnType<typeof setTimeout> | null = null;

	/**
	 * Copies a snippet and names which one was copied, so the feedback is on the button.
	 * The label goes back to "Copy" shortly after: a button that stays "Copied" reads as
	 * its permanent name, and gives no feedback the second time it is pressed.
	 */
	async function copy(label: string, text: string): Promise<void> {
		try {
			await copyText(text);
			copied = label;
			if (copiedTimer !== null) clearTimeout(copiedTimer);
			copiedTimer = setTimeout(() => {
				copiedTimer = null;
				copied = null;
			}, COPIED_MS);
		} catch (e) {
			push('error', `Could not copy: ${errorMessage(e)}`);
		}
	}

	$effect(() => () => {
		if (copiedTimer !== null) clearTimeout(copiedTimer);
	});

	/** Copies the server's values into the draft, discarding any unsaved edits. */
	function syncDraft(): void {
		const d = draftFromSettings();
		enabled = d.enabled;
		baseUrl = d.baseUrl;
		apiKey = d.apiKey;
		model = d.model;
		threshold = d.threshold;
	}

	async function save(): Promise<void> {
		// The diff lives in the store so it can be unit tested; a blank API key box means
		// "leave the stored key alone" and sends nothing for that key.
		const partial = changedSettings({ enabled, baseUrl, apiKey, model, threshold });
		if (Object.keys(partial).length === 0) {
			push('info', 'No changes to save');
			return;
		}
		// Captured before `syncDraft` blanks the box again: a stored key never reads back,
		// so this is the only moment the real value is still around to mirror.
		const savedKey = typeof partial['extraction.api_key'] === 'string' ? partial['extraction.api_key'] : null;
		saving = true;
		saveError = null;
		try {
			await saveSettings(partial);
			syncDraft();
			push('success', 'Settings saved');
			if (savedKey && vault === 'unlocked') {
				try {
					await mirrorKey('global', savedKey);
					await loadVault();
				} catch (e) {
					push('error', `Vault: ${errorMessage(e)}`);
				}
			}
		} catch (e) {
			// The daemon's `error` string is the whole explanation; show it verbatim.
			saveError = errorMessage(e);
			push('error', saveError);
		} finally {
			saving = false;
		}
	}

	async function reload(): Promise<void> {
		await loadSettings();
		syncDraft();
		syncAppearanceDraft();
		await loadStages();
		await loadMcp();
		await loadAutostart();
		await loadAbout();
		await loadVault();
		await loadNotifyPermission();
		await loadUiState();
	}

	async function loadStages(): Promise<void> {
		try {
			stages = (await api().boardStages()).stages;
			stagesError = null;
		} catch (e) {
			stages = [];
			stagesError = errorMessage(e);
		}
	}

	async function saveStages(next: Stage[], renames: Record<string, string>): Promise<void> {
		try {
			stages = await api().setBoardStages(next, renames);
			stagesError = null;
			push('success', 'Board stages saved');
		} catch (e) {
			stagesError = errorMessage(e);
			push('error', stagesError);
		}
	}

	// -- Appearance ---------------------------------------------------------------------
	//
	// Theme (Dark, Light, or an imported pack), fonts and the base UI size. Nothing here
	// touches the document, localStorage or the daemon until Save; the live preview
	// strip below reflects the draft immediately, scoped to itself via `data-theme` and
	// inline custom properties rather than the real window.

	let draftTheme = $state<Theme>('dark');
	let draftPack = $state<ThemePack | null>(null);
	let draftFontUi = $state<FontUi>('system');
	let draftFontMono = $state<FontMono>('jetbrains-mono');
	let draftFontSize = $state<FontSize>(12);

	let appearanceSaving = $state(false);
	let appearanceError = $state<string | null>(null);
	let importingTheme = $state(false);
	let importError = $state<string | null>(null);
	let fileInput = $state<HTMLInputElement | null>(null);

	/** The base theme plus, when one is imported, its own entry: only one pack can be
	 * stored at a time, so there is never more than one extra option here. */
	const themeOptions = $derived([
		{ value: 'dark', label: 'Dark' },
		{ value: 'light', label: 'Light' },
		...(draftPack ? [{ value: draftPack.name, label: draftPack.name }] : [])
	]);
	const selectedTheme = $derived(draftPack ? draftPack.name : draftTheme);
	/** What the preview strip (and, on Save, the real window) paints: a pack's own base
	 * wins, since its tokens are built against that ramp. */
	const previewBase = $derived(draftPack ? draftPack.base : draftTheme);
	const previewTokenStyle = $derived(
		draftPack ? Object.entries(draftPack.tokens).map(([k, v]) => `${k}:${v}`).join(';') : ''
	);

	/** Picking Dark or Light drops the imported pack; picking the pack's own name (the
	 * only other option) is a no-op, since it is already selected. */
	function onThemeChange(value: string): void {
		if (value === 'dark' || value === 'light') {
			draftTheme = value;
			draftPack = null;
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

	function syncAppearanceDraft(): void {
		draftTheme = settingString('ui.theme') === 'light' ? 'light' : 'dark';
		const packRaw = settingString(UI_THEME_PACK_KEY);
		draftPack = packRaw ? parseStoredPack(packRaw) : null;
		const fontUi = settingString(UI_FONT_UI_KEY);
		draftFontUi = fontUi === 'inter' || fontUi === 'jetbrains-mono' ? fontUi : 'system';
		draftFontMono = settingString(UI_FONT_MONO_KEY) === 'system-mono' ? 'system-mono' : 'jetbrains-mono';
		const size = settingNumber(UI_FONT_SIZE_KEY, 12);
		draftFontSize = size === 11 || size === 13 ? size : 12;
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
				[UI_FONT_SIZE_KEY]: draftFontSize
			};
			await api().setSettings(partial);
			await loadSettings();
			applyAppearance(base, draftPack, draftFontUi, draftFontMono, draftFontSize);
			push('success', 'Appearance saved');
		} catch (e) {
			appearanceError = errorMessage(e);
			push('error', appearanceError);
		} finally {
			appearanceSaving = false;
		}
	}

	/** Saves the form first when it is dirty, then calls `/extraction/test`. */
	async function testConnection(): Promise<void> {
		if (Object.keys(changedSettings({ enabled, baseUrl, apiKey, model, threshold })).length > 0) {
			await save();
			if (saveError) return;
		}
		testing = true;
		testResult = null;
		try {
			testResult = await api().testExtraction();
			if (testResult.ok) push('success', `Connected. Reply: ${testResult.reply}`);
			else push('error', testResult.error ?? 'Connection failed');
		} catch (e) {
			const message = errorMessage(e);
			testResult = { ok: false, error: message };
			push('error', message);
		} finally {
			testing = false;
		}
	}

	onMount(() => {
		void reload();
	});

	// `/settings#<section>` (the side panel's Sections rows) scrolls to that card once its
	// content has loaded. It reads the hash rather than running once, so a second row
	// picked while this page is already open scrolls too.
	$effect(() => {
		scrollToSection(page.url.hash, settings.loaded, (id) => document.getElementById(id));
	});

	$effect(() => {
		setStatusItems({ right: [{ text: 'settings' }] });
	});
</script>

<div class="title-row"><span class="title">Settings</span></div>

{#if settings.error && !settings.loaded}
	<ErrorState message={settings.error} logPath={settings.errorLogPath ?? undefined}>
		<Button variant="primary" onclick={reload}>Retry</Button>
	</ErrorState>
{:else}
	<div class="pane" data-testid="settings-form">
		<section class="card" id="daemon">
			<div class="card-head"><span class="card-title">Daemon</span></div>
			<div class="card-body">
				<div class="pair">
					<Input label="Port" mono readonly value={port} data-testid="settings-port" />
					<Input
						label="Embedding model"
						mono
						readonly
						value={embeddingModel}
						data-testid="settings-embedding"
					/>
				</div>
				<span class="hint">Both are set when the daemon starts and are shown here for reference.</span>
				<Checkbox
					label="Start Atlas at login"
					checked={autostart}
					disabled={!inTauri() || autostartBusy}
					title={inTauri() ? undefined : 'Only available in the desktop app'}
					data-testid="settings-autostart"
					onchange={() => toggleAutostart(!autostart)}
				/>
			</div>
		</section>

		<section class="card" id="extraction">
			<div class="card-head">
				<span class="card-title">Extraction</span>
				<span class="spacer"></span>
				{#if testResult}
					<span
						class="test-result"
						class:bad={!testResult.ok}
						role="status"
						data-testid="extraction-test-result"
					>
						{testResult.ok ? `Connected. Reply: ${testResult.reply}` : testResult.error}
					</span>
				{/if}
				<Button
					variant="ghost"
					size="sm"
					data-testid="settings-test"
					disabled={testDisabled}
					title={testDisabled
						? 'Enable extraction and set a base URL and model first'
						: 'Send a test request to the configured endpoint'}
					onclick={testConnection}
				>
					{testing ? 'Testing…' : 'Test connection'}
				</Button>
			</div>

			<div class="card-body">
				<Checkbox label="Enable extraction" bind:checked={enabled} data-testid="settings-enabled" />

				<div class="pair">
					<Input
						label="Base URL"
						mono
						bind:value={baseUrl}
						data-testid="settings-base-url"
						placeholder="https://api.deepseek.com"
					/>
					<Input
						label="Model"
						mono
						bind:value={model}
						data-testid="settings-model"
						placeholder="deepseek-chat"
					/>
				</div>

				<Input
					label="API key"
					type="password"
					mono
					bind:value={apiKey}
					autocomplete="off"
					data-testid="settings-api-key"
					placeholder={keyStored ? 'unchanged' : 'sk-…'}
					hint={keyStored ? 'A key is stored. Leave this blank to keep it.' : 'No key stored yet.'}
				/>
				{#if keyWillBeCleared}
					<span class="hint warn" role="status" data-testid="settings-key-cleared-note">
						Changing the base URL clears the stored key; enter it again.
					</span>
				{/if}
				{#if inTauri() && vaultHintText(vault)}
					<span class="hint" role="status" data-testid="settings-vault-hint">
						{vaultHintText(vault)}
					</span>
				{/if}

				<div class="field">
					<span class="label">Auto-accept threshold</span>
					<div class="slider">
						<input
							type="range"
							min="0"
							max="1"
							step="0.05"
							bind:value={threshold}
							aria-label="Auto-accept threshold"
							data-testid="settings-threshold"
						/>
						<output class="mono" data-testid="settings-threshold-value">
							{threshold.toFixed(2)}
						</output>
					</div>
					<span class="hint">Review's "Accept all above threshold" uses this value.</span>
				</div>

				<span class="hint" data-testid="extraction-key-note">
					The API key is stored in the local Atlas database, sent only to the base URL above,
					and never logged.
				</span>

				{#if saveError}
					<p class="bad" role="alert" data-testid="settings-error">{saveError}</p>
				{/if}
			</div>
		</section>

		<section class="card" id="board-stages">
			<div class="card-head"><span class="card-title">Board stages</span></div>
			<div class="card-body">
				<span class="hint">
					The columns every project uses unless it sets its own. Renaming a stage here moves
					the tasks standing in it.
				</span>
				{#if stagesError}
					<p class="bad" role="alert" data-testid="board-stages-error">{stagesError}</p>
				{/if}
				<StageEditor {stages} onsave={saveStages} />
			</div>
		</section>

		<section class="card" id="appearance">
			<div class="card-head">
				<span class="card-title">Appearance</span>
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
			</div>
			<div class="card-body">
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
			</div>
		</section>

		<section class="card" id="mcp">
			<div class="card-head">
				<span class="card-title">MCP server</span>
				<Badge tone={online ? 'success' : 'neutral'} icon={online ? 'circle-check' : 'circle'}>
					{online ? 'running' : 'offline'}
				</Badge>
				<span class="mono count" data-testid="mcp-counts">{mcpCountsText}</span>
				<span class="spacer"></span>
				<Button
					variant="ghost"
					size="sm"
					data-testid="mcp-copy-claude"
					onclick={() => copy('claude', CLAUDE_SNIPPET)}
				>
					{copied === 'claude' ? 'Copied' : 'Copy Claude command'}
				</Button>
				<Button
					variant="ghost"
					size="sm"
					data-testid="mcp-copy-codex"
					onclick={() => copy('codex', CODEX_SNIPPET)}
				>
					{copied === 'codex' ? 'Copied' : 'Copy Codex config'}
				</Button>
				<Button
					variant="ghost"
					size="sm"
					data-testid="mcp-restart"
					disabled={!inTauri() || restarting}
					title={inTauri() ? 'Stop and start the daemon' : 'Only available in the desktop app'}
					onclick={restartDaemon}
				>
					{restarting ? 'Restarting…' : 'Restart'}
				</Button>
			</div>

			<div class="card-body">
				{#if mcpError}
					<p class="bad" role="alert" data-testid="mcp-error">{mcpError}</p>
				{/if}

				<div class="pair">
					<div class="group">
						<span class="group-heading">Transports</span>
						<div class="transport">
							<span class="transport-name">stdio</span>
							<span class="mono value">{mcp?.transports.stdio.command ?? 'atlas mcp'}</span>
							<span class="spacer"></span>
							<Badge>default</Badge>
						</div>
						<div class="transport">
							<span class="transport-name">HTTP</span>
							<span class="mono value">
								{mcp?.transports.http.url ?? `http://127.0.0.1:${port}/mcp`}
							</span>
							<span class="spacer"></span>
							<Badge>loopback only</Badge>
						</div>
						<div class="transport" data-testid="mcp-protocol">
							<span class="transport-name">Protocol</span>
							<span class="mono value">{mcp?.transports.http.protocol_version ?? '…'}</span>
							<span class="spacer"></span>
							<span class="hint">tools · resources · prompts</span>
						</div>
					</div>

					<div class="group">
						<span class="group-heading">Connect</span>
						<pre class="mono connect-snippet">{connectSnippet}</pre>
					</div>
				</div>

				<div class="group">
					<span class="group-heading">Connected clients</span>
					<div class="clients-table" data-testid="mcp-clients">
						<Table
							id="mcp-clients"
							columns={clientColumns}
							rows={mcp?.clients ?? []}
							rowKey={(c: McpClient) => c.id}
						>
							{#snippet cell(clientRow: McpClient, column: TableColumn<McpClient>)}
								{#if column.key === 'last_seen'}
									{relativeAge(clientRow.last_seen)} ago
								{:else if column.key === 'transport'}
									{clientRow.transport}
								{:else if column.key === 'tool_calls'}
									{clientRow.tool_calls}
								{:else}
									{clientRow.client_name}
								{/if}
							{/snippet}
							{#snippet empty()}
								<span class="hint">No clients connected right now.</span>
							{/snippet}
						</Table>
					</div>
					<span class="hint">
						A stdio client's call count refreshes on its 60 s heartbeat, so it can lag behind
						the calls it has actually made.
					</span>
				</div>

				<div class="group">
					<span class="group-heading">Tools</span>
					<div class="tools-table" data-testid="mcp-tools">
						<Table
							id="mcp-tools"
							columns={toolColumns}
							rows={mcp?.tools ?? []}
							rowKey={(t: McpToolRow) => t.name}
						>
							{#snippet cell(toolRow: McpToolRow, column: TableColumn<McpToolRow>)}
								{#if column.key === 'name'}
									<span class="tool-name">
										<Icon name={toolIcon(toolRow.name)} size={12} color="var(--text-tertiary)" />
										<span class="mono">{toolRow.name}</span>
									</span>
								{:else if column.key === 'description'}
									{toolRow.description}
								{:else if column.key === 'args'}
									{toolRow.args}
								{:else if column.key === 'scope'}
									<Badge tone={toolRow.scope === 'write' ? 'warning' : 'info'}>
										{toolRow.scope}
									</Badge>
								{:else}
									<Checkbox
										checked={toolRow.enabled}
										disabled={togglingTool === toolRow.name}
										aria-label={`Enable ${toolRow.name}`}
										data-testid={`mcp-tool-toggle-${toolRow.name}`}
										onchange={() => toggleTool(toolRow)}
									/>
								{/if}
							{/snippet}
							{#snippet empty()}
								<span class="hint">No tools reported yet.</span>
							{/snippet}
						</Table>
					</div>
					<span class="hint">
						Disabled tools are absent from <span class="mono">tools/list</span> and a call to one
						answers "method not found". Write tools still respect a project's agent access rules.
					</span>
				</div>

				<div class="pair">
					<div class="group">
						<span class="group-heading">Resources</span>
						<div class="resource-list">
							{#each mcp?.resources ?? [] as resource (resource.uri)}
								<div class="resource-row">
									<span class="mono value">{resource.uri}</span>
									<span class="hint">{resource.description ?? ''}</span>
								</div>
							{/each}
						</div>
					</div>
					<div class="group">
						<span class="group-heading">Prompts</span>
						<div class="resource-list">
							{#each mcp?.prompts ?? [] as prompt (prompt.name)}
								<div class="resource-row">
									<span class="mono value">{prompt.name}</span>
									<span class="hint">{prompt.description ?? ''}</span>
								</div>
							{/each}
						</div>
					</div>
				</div>
			</div>
		</section>

		<section class="card" id="shortcuts">
			<div class="card-head"><span class="card-title">Shortcuts</span></div>
			<div class="card-body">
				<div class="group">
					<span class="group-heading">Global shortcut</span>
					<span class="hint">
						Works from anywhere, even when Atlas is not the focused app: shows the window
						and opens the command palette.
					</span>
					<div class="recorder-row">
						<button
							type="button"
							class="recorder"
							data-testid="shortcut-recorder"
							disabled={!inTauri()}
							title={inTauri() ? 'Click, then press a key combination' : 'Only available in the desktop app'}
							onclick={startRecording}
							onkeydown={onRecorderKeydown}
							onblur={onRecorderBlur}
						>
							{#if recorder.recording && !capturedAccelerator}
								Press a key combination…
							{:else if recorderCombo}
								<KeyHint combo={recorderCombo} />
							{:else}
								No shortcut set
							{/if}
						</button>
						<Button
							variant="primary"
							size="sm"
							data-testid="shortcut-apply"
							disabled={!capturedAccelerator || applyingShortcut}
							onclick={applyShortcut}
						>
							{applyingShortcut ? 'Applying…' : 'Apply'}
						</Button>
					</div>
					{#if shortcutError}
						<p class="bad" role="alert" data-testid="shortcut-error">{shortcutError}</p>
					{/if}
				</div>

				<div class="group">
					<span class="group-heading">In-app shortcuts</span>
					<div class="shortcut-list">
						{#each inAppShortcuts as s (s.combo)}
							<div class="shortcut-row">
								<span>{s.label}</span>
								<KeyHint combo={s.combo} plain />
							</div>
						{/each}
					</div>
				</div>
			</div>
		</section>

		<section class="card" id="notifications">
			<div class="card-head">
				<span class="card-title">Notifications</span>
				<span class="spacer"></span>
				<Button
					variant="ghost"
					size="sm"
					data-testid="settings-test-notification"
					disabled={sendingTestNotification}
					onclick={sendTestNotification}
				>
					{sendingTestNotification ? 'Sending…' : 'Send test notification'}
				</Button>
			</div>
			<div class="card-body">
				{#if inTauri() && notifyPermissionHint}
					<div class="recorder-row">
						<span class="hint" role="status" data-testid="settings-notify-permission">
							{notifyPermissionHint}
						</span>
						{#if notifyPermission === 'default'}
							<Button
								variant="ghost"
								size="sm"
								data-testid="settings-notify-request-permission"
								onclick={requestNotifyPermission}
							>
								Allow notifications
							</Button>
						{/if}
					</div>
				{/if}
				<Checkbox
					label="Memories waiting for review"
					checked={notifyFlag('ui.notify.review_pending')}
					data-testid="settings-notify-review-pending"
					onchange={() => setNotifyFlag('ui.notify.review_pending', !notifyFlag('ui.notify.review_pending'))}
				/>
				<Checkbox
					label="Workflow runs finished or failed"
					checked={notifyFlag('ui.notify.workflow_runs')}
					data-testid="settings-notify-workflow-runs"
					onchange={() => setNotifyFlag('ui.notify.workflow_runs', !notifyFlag('ui.notify.workflow_runs'))}
				/>
				<Checkbox
					label="Daemon unreachable"
					checked={notifyFlag('ui.notify.daemon_errors')}
					data-testid="settings-notify-daemon-errors"
					onchange={() => setNotifyFlag('ui.notify.daemon_errors', !notifyFlag('ui.notify.daemon_errors'))}
				/>
				<span class="hint">
					A background check every 30 seconds; each kind is off until you turn it on.
				</span>
			</div>
		</section>

		<section class="card" id="security">
			<div class="card-head">
				<span class="card-title">Security</span>
				<span class="spacer"></span>
				<Badge tone={vault === 'unlocked' ? 'success' : vault === 'locked' ? 'warning' : 'neutral'}>
					{vault}
				</Badge>
			</div>
			<div class="card-body">
				<span class="hint">
					An encrypted vault (<span class="mono">atlas.hold</span>) in the app data folder that
					mirrors every extraction API key you save here. The daemon keeps its own copy in
					DuckDB, so extraction still works headless without the vault unlocked.
				</span>
				<Input
					label="Passphrase"
					type="password"
					autocomplete="off"
					bind:value={vaultPassphrase}
					disabled={!inTauri() || vaultBusy}
					data-testid="vault-passphrase"
				/>
				<div class="recorder-row">
					{#if vault === 'missing'}
						<Button
							variant="primary"
							size="sm"
							data-testid="vault-set"
							disabled={!inTauri() || vaultBusy || !vaultPassphrase}
							onclick={setVaultPassphrase}
						>
							{vaultBusy ? 'Setting…' : 'Set vault passphrase'}
						</Button>
					{:else if vault === 'locked'}
						<Button
							variant="primary"
							size="sm"
							data-testid="vault-unlock"
							disabled={!inTauri() || vaultBusy || !vaultPassphrase}
							onclick={unlockVault}
						>
							{vaultBusy ? 'Unlocking…' : 'Unlock vault'}
						</Button>
					{:else}
						<Button size="sm" data-testid="vault-lock" disabled={vaultBusy} onclick={lockVault}>
							{vaultBusy ? 'Locking…' : 'Lock'}
						</Button>
					{/if}
				</div>
				{#if vaultError}
					<p class="bad" role="alert" data-testid="vault-error">{vaultError}</p>
				{/if}
				{#if vault === 'unlocked'}
					<div class="group">
						<span class="group-heading">Stored keys</span>
						{#if vaultScopes.length === 0}
							<span class="hint">No keys mirrored yet.</span>
						{:else}
							{#each vaultScopes as scope (scope)}
								<div class="transport">
									<span class="mono value">{scope}</span>
									<span class="spacer"></span>
									<Button
										variant="ghost"
										size="sm"
										data-testid={`vault-reapply-${scope}`}
										disabled={vaultBusy}
										onclick={() => reapplyVaultKey(scope)}
									>
										Reapply
									</Button>
								</div>
							{/each}
						{/if}
					</div>
				{/if}
			</div>
		</section>

		<section class="card" id="about">
			<div class="card-head">
				<span class="card-title">About</span>
				<span class="spacer"></span>
				<Button
					variant="ghost"
					size="sm"
					data-testid="settings-copy-diagnostics"
					disabled={!about}
					onclick={() => copy('diagnostics', diagnostics)}
				>
					{copied === 'diagnostics' ? 'Copied' : 'Copy diagnostics'}
				</Button>
				<Button
					variant="ghost"
					size="sm"
					data-testid="settings-open-log-folder"
					disabled={!inTauri()}
					title={inTauri() ? 'Reveal the log folder' : 'Only available in the desktop app'}
					onclick={openLogFolder}
				>
					Open log folder
				</Button>
			</div>
			<div class="card-body">
				{#if !inTauri()}
					<p class="hint" data-testid="about-browser-hint">Only available in the desktop app.</p>
				{:else if aboutError}
					<p class="bad" role="alert" data-testid="about-error">{aboutError}</p>
				{:else if !about}
					<p class="hint">Loading…</p>
				{/if}
				<dl class="about-list" data-testid="about-info">
					{#if about}
						<dt>App version</dt>
						<dd class="mono">{about.app_version}</dd>
						<dt>Tauri version</dt>
						<dd class="mono">{about.tauri_version}</dd>
					{/if}
					<dt>Daemon version</dt>
					<dd class="mono">{status.report?.version ?? '…'}</dd>
					{#if about}
						<dt>OS</dt>
						<dd class="mono">{about.os_type} {about.os_version} ({about.arch})</dd>
						<dt>Locale</dt>
						<dd class="mono">{about.locale ?? NOTHING}</dd>
					{/if}
					<dt>Database</dt>
					<dd class="mono">{status.report?.db_path ?? '…'}</dd>
					{#if about}
						<dt>Log folder</dt>
						<dd class="mono">{about.log_dir}</dd>
						<dt>Data folder</dt>
						<dd class="mono">{about.data_dir}</dd>
					{/if}
				</dl>

				<div class="group">
					<span class="group-heading">Updates</span>
					<div class="recorder-row">
						<Button
							variant="ghost"
							size="sm"
							data-testid="update-check"
							disabled={!inTauri() || updateChecking || updateInstalling}
							onclick={checkForUpdates}
						>
							{updateChecking ? 'Checking…' : 'Check for updates'}
						</Button>
						{#if updateError}
							<span class="hint warn" role="status" data-testid="update-error">{updateError}</span>
						{:else if updateResult}
							<span class="hint" role="status" data-testid="update-result">
								{updateResult.available ? `Update available: ${updateResult.version}` : 'Atlas is up to date.'}
							</span>
						{/if}
					</div>
					{#if updateResult?.available}
						<div class="recorder-row">
							<Button
								variant="primary"
								size="sm"
								data-testid="update-install"
								disabled={updateInstalling}
								onclick={installUpdate}
							>
								{updateInstalling ? 'Installing…' : 'Download and install'}
							</Button>
							<Button
								variant="ghost"
								size="sm"
								data-testid="update-restart"
								disabled={updateInstalling}
								onclick={restartToUpdate}
							>
								Restart to update
							</Button>
						</div>
						{#if updateProgress}
							<div class="progress-track" data-testid="update-progress">
								<div class="progress-fill" style={`width: ${updateProgressPct}%`}></div>
							</div>
						{/if}
					{/if}
					{#if updateError === 'updates are not configured'}
						<span class="hint">
							See <a
								href="https://github.com/DarshanaWT/atlas/blob/main/docs/usage.md#desktop-platform"
								target="_blank"
								rel="noreferrer">docs/usage.md, Desktop platform</a
							> for the manual signing key steps.
						</span>
					{/if}
				</div>
			</div>
		</section>

		<div class="foot">
			<Button variant="primary" data-testid="settings-save" disabled={saving} onclick={save}>
				{saving ? 'Saving…' : 'Save'}
			</Button>
			<Button onclick={reload} disabled={saving || settings.loading}>Reload</Button>
		</div>
	</div>
{/if}

<style>
	.title-row {
		display: flex;
		align-items: center;
		height: 28px;
		flex: 0 0 28px;
	}

	.title {
		font-size: 15px;
		font-weight: 600;
	}

	/* Frame 10's content panel scrolls; the shell's panel never does. */
	.pane {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	.card {
		flex: 0 0 auto;
		overflow: hidden;
	}

	.card-head {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.card-title {
		font-weight: 600;
	}

	.card-body {
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}

	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.label {
		color: var(--text-secondary);
	}

	.slider {
		display: flex;
		align-items: center;
		gap: 10px;
		height: 28px;
	}

	.slider input {
		flex: 1;
		accent-color: var(--accent);
	}

	.slider output {
		min-width: 4ch;
		text-align: right;
	}

	.group {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.transport {
		display: flex;
		align-items: center;
		gap: 12px;
		height: 22px;
		color: var(--text-secondary);
	}

	.transport-name {
		width: 70px;
	}

	/* RESOURCES and PROMPTS rows: a URI or prompt name can run much longer than a
	   transport value, and its description can too, so each row is its own two-column
	   grid (name/URI, then description) instead of sharing `.transport`'s fixed-height
	   flex row, which wrapped and overlapped once either side got long. */
	.resource-list {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.resource-row {
		display: grid;
		grid-template-columns: minmax(160px, 260px) 1fr;
		gap: 12px;
		align-items: baseline;
		color: var(--text-secondary);
	}

	.resource-row .hint {
		white-space: normal;
	}

	.value {
		color: var(--text-primary);
	}

	.connect-snippet {
		margin: 0;
		padding: 6px 8px;
		background: var(--bg-base);
		border: 1px solid var(--border-default);
		border-radius: 3px;
		font-size: 12px;
		line-height: 18px;
		color: var(--text-secondary);
		white-space: pre;
		overflow-x: auto;
	}

	.count {
		color: var(--text-tertiary);
	}

	.tool-name {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	/* Bounds the tools table so 29 rows scroll inside the card instead of stretching it
	   past the pane; the clients table stays natural height since it is usually short. */
	.tools-table {
		height: 320px;
		display: flex;
		flex-direction: column;
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		overflow: hidden;
	}

	.clients-table {
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		overflow: hidden;
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

	/* Same size as the hint it sits under; only the colour says it is a warning. */
	.hint.warn {
		color: var(--danger-text);
	}

	.test-result {
		color: var(--accent);
	}

	.test-result.bad {
		color: var(--danger-text);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}

	.foot {
		display: flex;
		gap: 8px;
		flex: 0 0 auto;
		padding-bottom: 4px;
	}

	.recorder-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.recorder {
		flex: 1;
		height: 28px;
		padding: 0 10px;
		display: flex;
		align-items: center;
		border: 1px solid var(--border-default);
		border-radius: 3px;
		background: var(--bg-base);
		color: var(--text-secondary);
		font-size: 12px;
		text-align: left;
		cursor: pointer;
	}

	.recorder:disabled {
		cursor: default;
		opacity: 0.5;
	}

	.recorder:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: -1px;
	}

	.shortcut-list {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.shortcut-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		height: 22px;
		color: var(--text-secondary);
	}

	.about-list {
		display: grid;
		grid-template-columns: 140px 1fr;
		row-gap: 6px;
		column-gap: 12px;
	}

	.about-list dt {
		color: var(--text-tertiary);
	}

	.about-list dd {
		margin: 0;
		color: var(--text-primary);
		word-break: break-all;
	}

	.progress-track {
		height: 4px;
		border-radius: 2px;
		background: var(--bg-base);
		overflow: hidden;
	}

	.progress-fill {
		height: 100%;
		background: var(--accent);
		transition: width 0.2s ease;
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
