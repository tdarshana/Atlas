// The macOS permissions Atlas needs, as rows the Permissions view draws. Pure: the four
// definitions, the mapping from what `permissions_status` reports onto a badge, and the
// counts the Settings card and the side panel print. Nothing here talks to Tauri.

/** The four rows, by id. The Rust side in `src-tauri/src/commands/permissions.rs` names
 * the same strings. */
export type PermissionId = 'notifications' | 'automation-finder' | 'accessibility' | 'files';

/** What the OS says about one permission. `not_applicable` is a permission this platform
 * has no concept of; `unknown` is one the OS would not answer for. */
export type PermissionState =
	| 'granted'
	| 'denied'
	| 'not_determined'
	| 'unknown'
	| 'not_applicable';

const STATES: PermissionState[] = [
	'granted',
	'denied',
	'not_determined',
	'unknown',
	'not_applicable'
];

/** One row of `permissions_status`, straight off the wire. */
export interface PermissionStatus {
	id: string;
	granted: string;
	detail?: string | null;
}

export interface PermissionDef {
	id: PermissionId;
	name: string;
	/** Why Atlas needs it, in the user's terms. */
	why: string;
	/** A second line where the grant alone is not the whole story. */
	note?: string;
	/** The System Settings pane the `Open settings` action opens. */
	settingsUrl: string;
	/** True when the app can raise the system's own consent prompt for this one. */
	canPrompt: boolean;
	/**
	 * True when that prompt still works after a refusal. `AXIsProcessTrustedWithOptions`
	 * raises its "open System Settings" sheet whenever the process is untrusted, which is
	 * exactly the state Accessibility reports as `denied`; the Apple Event and notification
	 * grants instead answer a second ask with the decision already stored, so asking again
	 * would do nothing and the row sends the user to System Settings.
	 */
	promptWhenDenied?: boolean;
}

/**
 * The four permissions, in the order the view draws them. The URLs are the
 * `x-apple.systempreferences:` schemes macOS answers to; notifications has its own pane
 * rather than a `Privacy_` one, which is why its URL reads differently.
 */
export const PERMISSIONS: PermissionDef[] = [
	{
		id: 'notifications',
		name: 'Notifications',
		why: 'Review and workflow alerts.',
		settingsUrl: 'x-apple.systempreferences:com.apple.preference.notifications',
		canPrompt: true
	},
	{
		id: 'automation-finder',
		name: 'Automation for Finder',
		why: 'The dmg bundling step drives the Finder.',
		note: 'The terminal app that runs the build needs the same grant.',
		settingsUrl: 'x-apple.systempreferences:com.apple.preference.security?Privacy_Automation',
		canPrompt: true
	},
	{
		id: 'accessibility',
		name: 'Accessibility',
		why: 'The global shortcut while Atlas is in the background.',
		settingsUrl: 'x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility',
		canPrompt: true,
		promptWhenDenied: true
	},
	{
		id: 'files',
		name: 'Files and folders',
		why: 'Reading connected project roots and writing the app data directory.',
		settingsUrl:
			'x-apple.systempreferences:com.apple.preference.security?Privacy_FilesAndFolders',
		// macOS grants folder access when the app reads the folder; there is no prompt
		// Atlas can raise on its own, so the row offers only `Open settings`.
		canPrompt: false
	}
];

/** The badge label per state. Sentence case, as everywhere else. */
const BADGES: Record<PermissionState, string> = {
	granted: 'Granted',
	denied: 'Denied',
	not_determined: 'Not determined',
	unknown: 'Unknown',
	not_applicable: 'Not applicable'
};

type Tone = 'success' | 'danger' | 'warning' | 'neutral';

const TONES: Record<PermissionState, Tone> = {
	granted: 'success',
	denied: 'danger',
	not_determined: 'warning',
	unknown: 'neutral',
	not_applicable: 'neutral'
};

/** One row on screen: the definition, plus what the OS said about it. */
export interface PermissionRow extends PermissionDef {
	state: PermissionState;
	badge: string;
	tone: Tone;
	detail: string | null;
	/** Whether the `Request` button is offered: where the app can prompt and the answer is
	 * still open, plus the rows whose system prompt keeps working after a refusal. A denied
	 * Apple Event or notification grant is changed in System Settings, not by asking again,
	 * since macOS answers a second ask with the old decision. */
	canRequest: boolean;
}

/** A state string off the wire, or `unknown` when it is not one this app knows. */
export function toState(value: string | undefined | null): PermissionState {
	return STATES.includes(value as PermissionState) ? (value as PermissionState) : 'unknown';
}

/**
 * The rows to draw, one per definition and always in the definition order. A permission
 * the status call did not answer for reads as `unknown` rather than being left out: a
 * missing row would say the permission is not needed, which is a different claim.
 */
export function toRows(statuses: PermissionStatus[]): PermissionRow[] {
	const byId = new Map(statuses.map((s) => [s.id, s]));
	return PERMISSIONS.map((def) => {
		const status = byId.get(def.id);
		const state = toState(status?.granted);
		return {
			...def,
			state,
			badge: BADGES[state],
			tone: TONES[state],
			detail: status?.detail ?? null,
			canRequest:
				def.canPrompt &&
				(state === 'not_determined' ||
					state === 'unknown' ||
					(state === 'denied' && def.promptWhenDenied === true))
		};
	});
}

/** How many of the rows that apply to this platform are granted, for the Settings card
 * and the side panel. A `not_applicable` row is left out of both halves: counting it
 * would make the ratio look worse on a platform where the permission does not exist. */
export function grantedCount(rows: PermissionRow[]): { granted: number; total: number } {
	const applicable = rows.filter((r) => r.state !== 'not_applicable');
	return {
		granted: applicable.filter((r) => r.state === 'granted').length,
		total: applicable.length
	};
}

/** The Settings card's one-line summary. */
export function summaryText(rows: PermissionRow[], plugins: number): string {
	const { granted, total } = grantedCount(rows);
	const pluginPart = plugins === 1 ? '1 plugin' : `${plugins} plugins`;
	return `${granted} of ${total} system permissions granted · ${pluginPart} with grants`;
}
