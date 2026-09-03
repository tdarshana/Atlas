// The About card's "Copy diagnostics" text: a plain block a user can paste into a bug
// report, built from `about_info` plus the daemon's own version and database path.

import type { AboutInfo } from '$lib/types';

export interface DiagnosticsInfo extends AboutInfo {
	daemon_version: string;
	db_path: string;
}

/**
 * One line per fact, in the order the About card lists them, plus the persisted UI
 * state as one trailing JSON line when `uiState` has anything in it: `ui_state_all` has
 * no other caller, and a bug report often hinges on which rail, theme or filters were
 * active.
 */
export function diagnosticsText(info: DiagnosticsInfo, uiState?: Record<string, unknown>): string {
	const lines = [
		`Atlas ${info.app_version} (Tauri ${info.tauri_version})`,
		`OS: ${info.os_type} ${info.os_version} (${info.arch})`,
		`Locale: ${info.locale ?? 'unknown'}`,
		`Daemon: ${info.daemon_version}`,
		`Database: ${info.db_path}`,
		`Logs: ${info.log_dir}`,
		`Data: ${info.data_dir}`
	];
	if (uiState && Object.keys(uiState).length > 0) {
		lines.push(`UI state: ${JSON.stringify(uiState)}`);
	}
	return lines.join('\n');
}
