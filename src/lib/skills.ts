// Pure helpers for the Skills view, the project Skills tab and the Skills side panel:
// the client-side filter behind the search box and the Source select, the label and the
// badge tone a source draws with, the disabled-list diff an `Enabled here` checkbox
// sends, and the counts the side panel groups by. No Svelte here, so these are plain
// unit tests.

import type { Doc, Skill, SkillSource, SkillSummary } from './types';

/** The value the Source select uses for "no filter". */
export const ALL_SOURCES = 'all';

/**
 * The Source select's six choices. `claude` and `codex` each cover both the project
 * and the user root, because a person picking "Claude Code" means the tool, not one of
 * its two folders. `practice` is the standing rules, which sit in the same table since
 * 2026-09-07: a practice is guidance an agent reads, like a skill, that happens to live
 * in the managed instruction block rather than in a folder.
 */
export type SourceFilter = 'all' | 'native' | 'claude' | 'codex' | 'plugin' | 'practice';

/** Every filter but "All": the groups a row can actually belong to. */
export type SourceGroup = Exclude<SourceFilter, 'all'>;

export const SOURCE_GROUPS: { source: SourceGroup; label: string }[] = [
	{ source: 'native', label: 'Native' },
	{ source: 'claude', label: 'Claude Code' },
	{ source: 'codex', label: 'Codex' },
	{ source: 'plugin', label: 'Plugins' },
	{ source: 'practice', label: 'Practices' }
];

/** A row's source: one of the daemon's skill sources, or a practice folded in client-side. */
export type ListedSource = SkillSource | 'practice';

/** What the table draws: a skill summary, or a practice in the same shape. */
export type SkillRow = Omit<SkillSummary, 'source'> & { source: ListedSource };

/** What the detail panel shows: a skill, or a practice in the same shape (with its tags). */
export type OpenSkill = Omit<Skill, 'source'> & { source: ListedSource; tags?: string[] };

/** A practice row's id: the daemon addresses a practice by name, so the row does too. */
export const PRACTICE_PREFIX = 'practice:';

export function isPracticeId(id: string): boolean {
	return id.startsWith(PRACTICE_PREFIX);
}

/** The practice name inside a practice row id. */
export function practiceName(id: string): string {
	return id.slice(PRACTICE_PREFIX.length);
}

/** The first line of a practice body, which is what the table shows as its description. */
function practiceDescription(body: string): string {
	const line = body.split('\n').find((l) => l.trim() !== '')?.trim() ?? '';
	return line.replace(/^#+\s*/, '');
}

/** A practice as a table row: same columns as a skill, source `practice`. */
export function practiceRow(doc: Doc): SkillRow {
	return {
		id: PRACTICE_PREFIX + doc.name,
		source: 'practice',
		name: doc.name,
		description: practiceDescription(doc.body),
		scope: doc.project_id ? 'project' : 'global',
		project_id: doc.project_id,
		path: null,
		plugin: null,
		editable: true,
		updated_at: doc.updated_at,
		enabled_here: null
	};
}

/** A practice as the detail panel shows it: its body is the whole document. */
export function practiceSkill(doc: Doc): OpenSkill {
	return { ...practiceRow(doc), body: doc.body, files: [], tags: doc.tags };
}

export const SOURCE_OPTIONS: { value: SourceFilter; label: string }[] = [
	{ value: 'all', label: 'All' },
	...SOURCE_GROUPS.map((g) => ({ value: g.source as SourceFilter, label: g.label }))
];

/** The badge a row draws, one per wire source. */
const SOURCE_LABELS: Record<ListedSource, string> = {
	native: 'Native',
	'claude-project': 'Claude Code',
	'claude-user': 'Claude Code',
	'codex-project': 'Codex',
	'codex-user': 'Codex',
	plugin: 'Plugin',
	practice: 'Practice'
};

export function sourceLabel(source: ListedSource): string {
	return SOURCE_LABELS[source] ?? source;
}

/** Which select option a row belongs to. */
export function sourceGroup(source: ListedSource): SourceGroup {
	if (source === 'native') return 'native';
	if (source === 'plugin') return 'plugin';
	if (source === 'practice') return 'practice';
	return source.startsWith('claude-') ? 'claude' : 'codex';
}

/**
 * The rows the table draws: those whose name or description contains `text`, case
 * insensitively, and whose source belongs to `source`. Both filters are optional; an
 * empty or blank search matches everything.
 */
export function filterSkills(
	items: SkillRow[],
	text: string,
	source: SourceFilter = 'all'
): SkillRow[] {
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

export function skillCounts(items: SkillRow[]): SkillCounts {
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

/** "3 of 7 skills enabled for this project, 2 practices", for the project tab's summary
 * line. Practices have no switch, so they are counted beside the skills, not among them. */
export function enabledSummary(items: SkillRow[]): string {
	const skills = items.filter((s) => s.source !== 'practice');
	const practices = items.length - skills.length;
	const enabled = skills.filter((s) => s.enabled_here !== false).length;
	const noun = skills.length === 1 ? 'skill' : 'skills';
	const tail = practices === 0 ? '' : `, ${practices} ${practices === 1 ? 'practice' : 'practices'}`;
	return `${enabled} of ${skills.length} ${noun} enabled for this project${tail}`;
}

// ---- the SKILL.md frontmatter ------------------------------------------------------

export interface SplitBody {
	/** The whole fenced block, `---` lines included, or null when there is none. */
	frontmatter: string | null;
	/** Everything after it, which is what Preview renders. */
	markdown: string;
}

/**
 * Splits a `SKILL.md` into its leading YAML block and the Markdown after it. The block
 * counts only when the very first line is `---` and a later line closes it; an
 * unterminated block, or a `---` further down that is a horizontal rule, is body text
 * and comes back whole, so nothing is ever silently swallowed.
 *
 * Preview renders `markdown` alone: the name and the description are already in the
 * detail panel's header, and feeding the raw block to the renderer prints
 * `name: … description: …` as a paragraph running into the first heading.
 */
export function splitFrontmatter(body: string): SplitBody {
	const lines = body.split('\n');
	if (lines[0]?.trim() !== '---') return { frontmatter: null, markdown: body };

	for (let i = 1; i < lines.length; i++) {
		if (lines[i].trim() !== '---') continue;
		return {
			frontmatter: lines.slice(0, i + 1).join('\n'),
			// Drop the blank lines the block is usually followed by, so Preview does not
			// open on empty space.
			markdown: lines
				.slice(i + 1)
				.join('\n')
				.replace(/^\n+/, '')
		};
	}

	return { frontmatter: null, markdown: body };
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
