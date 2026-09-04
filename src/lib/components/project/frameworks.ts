// Pure helpers for the project Frameworks tab: the documents table's row shaping and
// badge tones, and the import report's toast text. Kept separate from the page so the
// shaping rules are unit-testable without mounting Svelte.

import type { FrameworkDoc, FrameworkDocType, FrameworkKind, FrameworkListing, ImportReport } from '$lib/types';

/** The subset of `Badge`'s `tone` prop the Type column uses. */
export type DocTypeTone = 'neutral' | 'accent' | 'success' | 'warning' | 'info';

/** Sentence-case display name for a framework kind, e.g. `superpowers` -> `Superpowers`. */
export const FRAMEWORK_LABEL: Record<FrameworkKind, string> = {
	superpowers: 'Superpowers',
	openspec: 'OpenSpec',
	speckit: 'SpecKit',
	gsd: 'GSD'
};

/** The Type column's badge tone, one per document kind a framework holds. */
export const DOC_TYPE_TONE: Record<FrameworkDocType, DocTypeTone> = {
	spec: 'info',
	plan: 'accent',
	tasks: 'warning',
	roadmap: 'accent',
	ledger: 'neutral',
	proposal: 'info',
	summary: 'success',
	todo: 'warning'
};

/** Sentence-case display label for a document type, e.g. `roadmap` -> `Roadmap`. */
export const DOC_TYPE_LABEL: Record<FrameworkDocType, string> = {
	spec: 'Spec',
	plan: 'Plan',
	tasks: 'Tasks',
	roadmap: 'Roadmap',
	ledger: 'Ledger',
	proposal: 'Proposal',
	summary: 'Summary',
	todo: 'Todo'
};

/** Every detected framework's documents in one list, newest first, for the tab's single
 * shared documents table. */
export function documentRows(listings: FrameworkListing[]): FrameworkDoc[] {
	return listings
		.flatMap((listing) => listing.documents)
		.sort((a, b) => (a.updated_at < b.updated_at ? 1 : a.updated_at > b.updated_at ? -1 : 0));
}

/** A document's row key for the Table component: kind and path together, since path
 * alone repeats across frameworks that share a layout. */
export function docRowKey(doc: FrameworkDoc): string {
	return `${doc.kind}:${doc.path}`;
}

/** The import report toast's text, e.g. `3 created, 1 updated, 12 skipped`. */
export function reportText(report: ImportReport): string {
	return `${report.created} created, ${report.updated} updated, ${report.skipped} skipped`;
}
