// The palette's little query language. `@name ` scopes the search to a project and
// stacks; a trailing `@partial` with no space after it is still being typed, so it
// opens the project picker instead. A leading `#`, `~` or `>` narrows the kind and is
// stripped from the text that goes to the daemon.

import type { Project, Uuid } from '$lib/types';

export type QueryType = 'all' | 'task' | 'memory' | 'command' | 'project-picker';

export interface ParsedQuery {
	/** Project names as typed, case preserved. Resolution happens separately. */
	scopes: string[];
	type: QueryType;
	/** What is left after the scopes and the type prefix are taken out. */
	text: string;
}

/** The three non-project prefixes, in the order the picker lists them. */
export const TYPE_PREFIXES: { prefix: string; type: QueryType }[] = [
	{ prefix: '#', type: 'task' },
	{ prefix: '~', type: 'memory' },
	{ prefix: '>', type: 'command' }
];

function typeOf(text: string): { type: QueryType; text: string } {
	const hit = TYPE_PREFIXES.find((p) => text.startsWith(p.prefix));
	if (!hit) return { type: 'all', text };
	return { type: hit.type, text: text.slice(hit.prefix.length).trim() };
}

export function parseQuery(input: string): ParsedQuery {
	const raw = input ?? '';
	const endsOpen = raw.length > 0 && !/\s$/.test(raw);
	const tokens = raw.split(/\s+/).filter(Boolean);

	const scopes: string[] = [];
	const rest: string[] = [];
	let partial: string | null = null;

	tokens.forEach((token, i) => {
		if (!token.startsWith('@')) {
			rest.push(token);
			return;
		}
		// The last token, still being typed: the picker, not a finished scope.
		if (i === tokens.length - 1 && endsOpen) {
			partial = token.slice(1);
			return;
		}
		const name = token.slice(1);
		if (name && !scopes.some((s) => s.toLowerCase() === name.toLowerCase())) scopes.push(name);
	});

	if (partial !== null) return { scopes, type: 'project-picker', text: partial };

	const { type, text } = typeOf(rest.join(' '));
	return { scopes, type, text };
}

/**
 * The input with every finished `@name ` token removed, so the box shows only the
 * text while the scopes render as chips beside it.
 */
export function stripScopes(input: string): string {
	const raw = input ?? '';
	const endsOpen = raw.length > 0 && !/\s$/.test(raw);
	const tokens = raw.split(/\s+/).filter(Boolean);
	const kept = tokens.filter(
		(token, i) => !token.startsWith('@') || (i === tokens.length - 1 && endsOpen)
	);
	return kept.join(' ');
}

/** One run of a title, either inside a match or outside it. */
export interface HighlightSegment {
	text: string;
	mark: boolean;
}

/**
 * Splits a title on the daemon's `highlights`, which are char offsets (not byte
 * offsets) into `title`, so a title with astral characters still lines up. Ranges are
 * sorted and clamped into what is left of the string, so a negative, reversed,
 * overlapping or past-the-end range from the daemon degrades to plain text.
 */
export function highlightSegments(
	title: string,
	highlights: [number, number][]
): HighlightSegment[] {
	const text = title ?? '';
	const ranges = Array.isArray(highlights) ? highlights : [];
	if (!ranges.length) return [{ text, mark: false }];

	const chars = Array.from(text);
	const out: HighlightSegment[] = [];
	let at = 0;
	for (const [s, e] of [...ranges].sort((a, b) => a[0] - b[0])) {
		const start = Math.max(at, Math.min(s, chars.length));
		const end = Math.max(start, Math.min(e, chars.length));
		if (start > at) out.push({ text: chars.slice(at, start).join(''), mark: false });
		if (end > start) out.push({ text: chars.slice(start, end).join(''), mark: true });
		at = end;
	}
	if (at < chars.length) out.push({ text: chars.slice(at).join(''), mark: false });
	return out;
}

/** A scope as the input renders it: resolved to a project, or unknown and ignored. */
export interface ScopeChip {
	name: string;
	id: Uuid | null;
}

/** Matches scope names against connected projects, case-insensitively. */
export function resolveScopes(scopes: string[], projects: Project[]): ScopeChip[] {
	return scopes.map((name) => {
		const hit = projects.find((p) => p.name.toLowerCase() === name.toLowerCase());
		return { name, id: hit ? hit.id : null };
	});
}
