// The global shortcut recorder: turns a captured key combo into the accelerator string
// `shortcut_set` and the `ui.global_shortcut` setting want (`CmdOrCtrl+Shift+K`), and
// formats one back for display as a `Mod`-style combo `KeyHint` understands.

/** The keys that are themselves modifiers, so pressing only one of them captures no key
 * yet: the recorder is still waiting for the key that completes the combo. */
const MODIFIER_KEYS = new Set(['Control', 'Meta', 'Alt', 'Shift']);

export interface CapturedCombo {
	/** In a fixed order (`CmdOrCtrl`, then `Alt`, then `Shift`) so the same combo always
	 * formats to the same string regardless of the order the keys were pressed in. */
	modifiers: string[];
	/** `null` while only modifiers are held down. */
	key: string | null;
}

/** A single-character key becomes its upper-case letter or digit; a few named keys are
 * spelled out the way Tauri's accelerator parser and `KeyHint` both expect. */
function keyName(key: string): string {
	if (key === ' ') return 'Space';
	if (key.length === 1) return key.toUpperCase();
	return key;
}

/**
 * Reads a captured combo off a keyboard event. Platform Cmd (mac) and Ctrl (Windows,
 * Linux) both map to `CmdOrCtrl`, so the same accelerator string works everywhere; a
 * combo held down without a non-modifier key yet has `key: null`.
 */
export function comboFromEvent(e: {
	metaKey: boolean;
	ctrlKey: boolean;
	altKey: boolean;
	shiftKey: boolean;
	key: string;
}): CapturedCombo {
	const modifiers: string[] = [];
	if (e.metaKey || e.ctrlKey) modifiers.push('CmdOrCtrl');
	if (e.altKey) modifiers.push('Alt');
	if (e.shiftKey) modifiers.push('Shift');
	const key = MODIFIER_KEYS.has(e.key) ? null : keyName(e.key);
	return { modifiers, key };
}

/** The accelerator string to send to `shortcut_set`, or `null` while the combo has no
 * key yet or carries no modifier (a bare key is never a valid global shortcut). */
export function comboToAccelerator(combo: CapturedCombo): string | null {
	if (!combo.key || combo.modifiers.length === 0) return null;
	return [...combo.modifiers, combo.key].join('+');
}

/** An accelerator string (`CmdOrCtrl+Shift+K`) as the `Mod`-style combo `KeyHint`
 * renders (`Mod+Shift+K`), for showing a stored `ui.global_shortcut` value. */
export function acceleratorToKeyHintCombo(accelerator: string): string {
	return accelerator
		.split('+')
		.map((part) => (part === 'CmdOrCtrl' || part === 'CommandOrControl' ? 'Mod' : part))
		.join('+');
}

// -- Recorder state machine --------------------------------------------------------
//
// Pulled out as pure functions, rather than left as state mutations inline in the
// Settings page, so the one rule that matters here is directly testable: losing focus
// must not lose the captured combo. The Apply button sits right next to the recorder,
// and a plain click on it fires `blur` before `click` in a click-focuses-buttons
// engine (Chromium, Firefox); clearing the combo on blur made Apply unclickable on
// every attempt. Only `cancelRecording` (Escape, or a completed Apply) actually clears
// what was captured.

const EMPTY_COMBO: CapturedCombo = { modifiers: [], key: null };

export interface RecorderState {
	recording: boolean;
	combo: CapturedCombo;
}

export const INITIAL_RECORDER_STATE: RecorderState = { recording: false, combo: EMPTY_COMBO };

/** Click on the recorder: starts a fresh capture, discarding whatever was there. */
export function startRecording(): RecorderState {
	return { recording: true, combo: EMPTY_COMBO };
}

/** Losing focus: stops recording, keeps the combo. */
export function blurRecording(state: RecorderState): RecorderState {
	return { ...state, recording: false };
}

/** Escape, or a successful Apply: recording stops and the combo is discarded. */
export function cancelRecording(): RecorderState {
	return { recording: false, combo: EMPTY_COMBO };
}

/** A keydown while recording: ignored once recording has already stopped (a stray key
 * after blur must not silently re-arm the combo). */
export function captureKey(
	state: RecorderState,
	e: { metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean; key: string }
): RecorderState {
	if (!state.recording) return state;
	return { ...state, combo: comboFromEvent(e) };
}
