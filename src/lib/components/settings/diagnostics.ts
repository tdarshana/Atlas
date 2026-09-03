// The About card's "Copy diagnostics" text: a plain block a user can paste into a bug
// report, built from `about_info` plus the daemon's own version and database path.

import type { AboutInfo } from '$lib/types';

export interface DiagnosticsInfo extends AboutInfo {
	daemon_version: string;
	db_path: string;
}

/** One line per fact, in the order the About card lists them. */
export function diagnosticsText(info: DiagnosticsInfo): string {
	return [
		`Atlas ${info.app_version} (Tauri ${info.tauri_version})`,
		`OS: ${info.os_type} ${info.os_version} (${info.arch})`,
		`Locale: ${info.locale ?? 'unknown'}`,
		`Daemon: ${info.daemon_version}`,
		`Database: ${info.db_path}`,
		`Logs: ${info.log_dir}`,
		`Data: ${info.data_dir}`
	].join('\n');
}
