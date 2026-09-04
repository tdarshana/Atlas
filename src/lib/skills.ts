// Pure helpers for the Skills view, the project Skills tab and the Skills side panel:
// the client-side filter behind the search box and the Source select, the label and the
// badge tone a source draws with, the disabled-list diff an `Enabled here` checkbox
// sends, and the counts the side panel groups by. No Svelte here, so these are plain
// unit tests.

import type { SkillSource, SkillSummary } from './types';

/** The value the Source select uses for "no filter". */
export const ALL_SOURCES = 'all';

/**
 * The Source select's five choices. `claude` and `codex` each cover both the project
 * and the user root, because a person picking "Claude Code" means the tool, not one of
 * its two folders.
 */
export type SourceFilter = 'all' | 'native' | 'claude' | 'codex' | 'plugin';

/** Every filter but "All": the groups a row can actually belong to. */
export type SourceGroup = Exclude<SourceFilter, 'all'>;

export const SOURCE_GROUPS: { source: SourceGroup; label: string }[] = [
	{ source: 'native', label: 'Native' },
	{ source: 'claude', label: 'Claude Code' },
	{ source: 'codex', label: 'Codex' },
	{ source: 'plugin', label: 'Plugins' }
];

export const SOURCE_OPTIONS: { value: SourceFilter; label: string }[] = [
	{ value: 'all', label: 'All' },
	...SOURCE_GROUPS.map((g) => ({ value: g.source as SourceFilter, label: g.label }))
];

/** The badge a row draws, one per wire source. */
const SOURCE_LABELS: Record<SkillSource, string> = {
	native: 'Native',
	'claude-project': 'Claude Code',
	'claude-user': 'Claude Code',
	'codex-project': 'Codex',
	'codex-user': 'Codex',
	plugin: 'Plugin'
};

export function sourceLabel(source: SkillSource): string {
	return SOURCE_LABELS[source] ?? source;
}

/** Which select option a row belongs to. */
export function sourceGroup(source: SkillSource): SourceGroup {
	if (source === 'native') return 'native';
	if (source === 'plugin') return 'plugin';
	return source.startsWith('claude-') ? 'claude' : 'codex';
}

/**
 * The rows the table draws: those whose name or description contains `text`, case
 * insensitively, and whose source belongs to `source`. Both filters are optional; an
 * empty or blank search matches everything.
 */
export function filterSkills(
	items: SkillSummary[],
	text: string,
	source: SourceFilter = 'all'
): SkillSummary[] {
	const needle = text.trim().toLowerCase();
	return items.filter((s) => {
		if (source !== 'all' && sourceGroup(s.source) !== source) return false;
		if (!needle) return true;
		return (
			s.name.toLowerCase().includes(needle) || s.description.toLowerCase().includes(needle)
		);
	});
}

/**
 * The whole `disabled` list to send after one `Enabled here` checkbox moves: `id`
 * removed when the skill was just enabled, added when it was just disabled. Everything
 * else in `current` is left alone, so a project's other overrides survive the write.
 */
export function nextDisabled(current: string[], id: string, enabled: boolean): string[] {
	if (enabled) return current.filter((x) => x !== id);
	return current.includes(id) ? current : [...current, id];
}

/** What the side panel groups by: a count per source, and a count per plugin. */
export interface SkillCounts {
	total: number;
	bySource: { source: SourceGroup; label: string; count: number }[];
	byPlugin: { plugin: string; count: number }[];
}

export function skillCounts(items: SkillSummary[]): SkillCounts {
	const bySource = SOURCE_GROUPS.map((g) => ({
		source: g.source,
		label: g.label,
		count: items.filter((s) => sourceGroup(s.source) === g.source).length
	}));

	const plugins = new Map<string, number>();
	for (const s of items) {
		if (!s.plugin) continue;
		plugins.set(s.plugin, (plugins.get(s.plugin) ?? 0) + 1);
	}

	return {
		total: items.length,
		bySource,
		byPlugin: [...plugins.entries()]
			.map(([plugin, count]) => ({ plugin, count }))
			.sort((a, b) => a.plugin.localeCompare(b.plugin))
	};
}

/** "3 of 7 skills enabled for this project", for the project tab's summary line. */
export function enabledSummary(items: SkillSummary[]): string {
	const enabled = items.filter((s) => s.enabled_here !== false).length;
	const noun = items.length === 1 ? 'skill' : 'skills';
	return `${enabled} of ${items.length} ${noun} enabled for this project`;
}

// ---- the detail panel's width ----------------------------------------------------

export const DETAIL_KEY = 'atlas.skills.detail';
export const DETAIL_DEFAULT = 380;
export const DETAIL_MIN = 300;
export const DETAIL_MAX = 640;

export function clampDetail(width: number): number {
	return Number.isFinite(width)
		? Math.min(DETAIL_MAX, Math.max(DETAIL_MIN, Math.round(width)))
		: DETAIL_DEFAULT;
}
