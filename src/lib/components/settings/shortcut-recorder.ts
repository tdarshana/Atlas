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
