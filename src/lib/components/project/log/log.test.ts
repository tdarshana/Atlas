import { describe, expect, it } from 'vitest';

import { logTime } from '$lib/format';
import { distinct, hasFilters, logParams, LOG_PAGE } from '$lib/stores/log.svelte';
import type { LogEntry, LogRefType } from '$lib/types';
import {
	eventsToday,
	exportFilename,
	kindOptions,
	refHref,
	refLabel,
	sourceOptions
} from './log';

/** 3 September 2026, 15:15 local time, the clock the frame is drawn at. */
const NOW = new Date(2026, 8, 3, 15, 15).getTime();

function entry(part: Partial<LogEntry> = {}): LogEntry {
	return {
		time: new Date(NOW).toISOString(),
		source: 'desktop',
		kind: 'moved',
		detail: 'moved ATL-2 from In Progress to Testing',
		ref: null,
		...part
	};
}

function ref(type: LogRefType, id: string, key?: string): LogEntry {
	return entry({ ref: { type, id, key: key ?? null } });
}

describe('logTime', () => {
	it('prints a clock for today', () => {
		expect(logTime(new Date(2026, 8, 3, 15, 15).toISOString(), NOW)).toBe('15:15');
	});

	it('pads a single digit hour so the column stays aligned', () => {
		expect(logTime(new Date(2026, 8, 3, 9, 5).toISOString(), NOW)).toBe('09:05');
	});

	it('names yesterday rather than dating it', () => {
		expect(logTime(new Date(2026, 8, 2, 23, 59).toISOString(), NOW)).toBe('Yesterday');
	});

	it('dates anything older', () => {
		expect(logTime(new Date(2026, 8, 1, 14, 12).toISOString(), NOW)).toBe('01/09');
		expect(logTime(new Date(2026, 7, 28, 9, 0).toISOString(), NOW)).toBe('28/08');
	});

	it('prints the null sentinel for an unreadable time', () => {
		expect(logTime('not a time', NOW)).toBe('·');
	});
});

describe('logParams', () => {
	it('sends nothing for a filter that is not set', () => {
		expect(logParams({ source: '', kind: '', q: '  ' })).toEqual({
			source: null,
			kind: null,
			q: null,
			after: null,
			limit: LOG_PAGE
		});
	});

	it('sends the filters that are set, trimmed', () => {
		expect(logParams({ source: 'cli/codex', kind: 'moved', q: '  ATL-2 ' })).toEqual({
			source: 'cli/codex',
			kind: 'moved',
			q: 'ATL-2',
			after: null,
			limit: LOG_PAGE
		});
	});

	it('carries `after` when paging', () => {
		const params = logParams({ source: '', kind: '', q: '' }, '2026-09-03T15:08:00Z');
		expect(params.after).toBe('2026-09-03T15:08:00Z');
	});

	it('knows when a filter is set', () => {
		expect(hasFilters({ source: '', kind: '', q: '' })).toBe(false);
		expect(hasFilters({ source: '', kind: '', q: '   ' })).toBe(false);
		expect(hasFilters({ source: 'desktop', kind: '', q: '' })).toBe(true);
		expect(hasFilters({ source: '', kind: 'moved', q: '' })).toBe(true);
		expect(hasFilters({ source: '', kind: '', q: 'x' })).toBe(true);
	});
});

describe('filter options', () => {
	it('offers the whole vocabulary under an all-sources row', () => {
		expect(sourceOptions(['desktop', 'cli/codex']).map((o) => o.label)).toEqual([
			'All sources',
			'cli/codex',
			'desktop'
		]);
		expect(sourceOptions(['desktop'])[0].value).toBe('');
	});

	it('offers the kinds under an all-events row', () => {
		expect(kindOptions(['moved', 'created']).map((o) => o.label)).toEqual([
			'All events',
			'created',
			'moved'
		]);
	});

	it('offers only the all row for an empty log', () => {
		expect(sourceOptions([])).toEqual([{ value: '', label: 'All sources' }]);
	});

	it('keeps offering the value in force even when the vocabulary does not hold it', () => {
		// Otherwise the select falls back to displaying "All sources" while the store still
		// sends `source=workflow`.
		expect(sourceOptions(['desktop'], 'workflow').map((o) => o.value)).toEqual([
			'',
			'desktop',
			'workflow'
		]);
	});

	it('does not repeat a selected value the vocabulary already holds', () => {
		expect(sourceOptions(['desktop'], 'desktop').map((o) => o.value)).toEqual(['', 'desktop']);
	});
});

describe('distinct', () => {
	it('is the vocabulary a page of rows produces, sorted and deduplicated', () => {
		const rows = [
			entry({ source: 'desktop', kind: 'moved' }),
			entry({ source: 'cli/codex', kind: 'moved' }),
			entry({ source: 'cli/codex', kind: 'created' })
		];
		expect(distinct(rows, (r) => r.source)).toEqual(['cli/codex', 'desktop']);
		expect(distinct(rows, (r) => r.kind)).toEqual(['created', 'moved']);
	});
});

describe('eventsToday', () => {
	it('counts by the calendar day, so a short or long day does not shift the window', () => {
		// Local components, not midnight plus 24 hours: a DST day is 23 or 25 hours long.
		const rows = [
			entry({ time: new Date(2026, 8, 3, 23, 59, 59).toISOString() }),
			entry({ time: new Date(2026, 8, 4, 0, 0, 1).toISOString() })
		];
		expect(eventsToday(rows, NOW)).toBe(1);
	});

	it('counts only the rows written today', () => {
		const rows = [
			entry({ time: new Date(2026, 8, 3, 15, 15).toISOString() }),
			entry({ time: new Date(2026, 8, 3, 0, 1).toISOString() }),
			entry({ time: new Date(2026, 8, 2, 23, 59).toISOString() })
		];
		expect(eventsToday(rows, NOW)).toBe(2);
	});
});

describe('refs', () => {
	it('links a task to the board, by key', () => {
		expect(refHref(ref('task', 'uuid-1', 'ATL-2'), 'p1')).toBe('/projects/p1/board?task=ATL-2');
		expect(refLabel(ref('task', 'uuid-1', 'ATL-2'))).toBe('ATL-2');
	});

	it('links a memory, a run, the project, a sync and a job', () => {
		expect(refHref(ref('memory', 'm1'), 'p1')).toBe('/memories?id=m1');
		expect(refHref(ref('run', 'r1'), 'p1')).toBe('/workflows');
		expect(refHref(ref('project', 'p1'), 'p1')).toBe('/projects/p1');
		expect(refHref(ref('sync', 'p1'), 'p1')).toBe('/projects/p1/agents');
		expect(refHref(ref('job', 'j1'), 'p1')).toBe('/projects/p1/settings');
	});

	it('has no link and no label for a row that points nowhere', () => {
		expect(refHref(entry(), 'p1')).toBeNull();
		expect(refLabel(entry())).toBe('');
	});

	it('shortens an id when the ref carries no key', () => {
		expect(refLabel(ref('memory', 'ea6af936-6170-4fb3-8d2b-622c1160e224'))).toBe('ea6af936');
	});
});

describe('exportFilename', () => {
	it('slugs the project name', () => {
		expect(exportFilename('atlas')).toBe('atlas-log.jsonl');
		expect(exportFilename('My AI Projects')).toBe('my-ai-projects-log.jsonl');
		expect(exportFilename('  Atlas/probe  ')).toBe('atlas-probe-log.jsonl');
	});

	it('falls back when the name slugs to nothing', () => {
		expect(exportFilename('///')).toBe('project-log.jsonl');
	});
});
