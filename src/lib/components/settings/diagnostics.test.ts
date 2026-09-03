import { describe, expect, it } from 'vitest';
import { diagnosticsText, type DiagnosticsInfo } from './diagnostics';

const info: DiagnosticsInfo = {
	app_version: '0.1.0',
	tauri_version: '2.11.5',
	os_type: 'macos',
	os_version: '15.1',
	arch: 'aarch64',
	locale: 'en-US',
	log_dir: '/Users/t/Library/Logs/atlas',
	data_dir: '/Users/t/Library/Application Support/atlas',
	daemon_version: '0.1.0',
	db_path: '/Users/t/.atlas/atlas.duckdb'
};

describe('diagnosticsText', () => {
	it('renders one line per fact in a fixed order', () => {
		expect(diagnosticsText(info)).toBe(
			[
				'Atlas 0.1.0 (Tauri 2.11.5)',
				'OS: macos 15.1 (aarch64)',
				'Locale: en-US',
				'Daemon: 0.1.0',
				'Database: /Users/t/.atlas/atlas.duckdb',
				'Logs: /Users/t/Library/Logs/atlas',
				'Data: /Users/t/Library/Application Support/atlas'
			].join('\n')
		);
	});

	it('falls back to "unknown" for a null locale', () => {
		expect(diagnosticsText({ ...info, locale: null })).toContain('Locale: unknown');
	});

	it('appends the UI state as one trailing JSON line when given', () => {
		expect(diagnosticsText(info, { theme: 'dark' })).toContain('UI state: {"theme":"dark"}');
	});

	it('omits the UI state line when it is missing or empty', () => {
		expect(diagnosticsText(info)).not.toContain('UI state:');
		expect(diagnosticsText(info, {})).not.toContain('UI state:');
	});
});
