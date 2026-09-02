// Review state: the pending memories an extractor proposed. Accept promotes one to
// `active`, Reject marks it `rejected`; either way the row leaves the list, because
// the list only ever shows what is still pending.

import { ApiError } from '$lib/api';
import { api, daemon } from '$lib/daemon.svelte';
import { minConfidence } from '$lib/stores/settings.svelte';
import type { Memory, Uuid } from '$lib/types';

export const review = $state({
	items: [] as Memory[],
	loading: false,
	error: null as string | null,
	/** Set only for a connection failure, so the error state can point at the log. */
	errorLogPath: null as string | null,
	/** The rows a decision is in flight for, so their buttons can disable. */
	busy: [] as Uuid[]
});

export async function loadReview(): Promise<void> {
	review.loading = true;
	try {
		review.items = await api().listMemories('pending');
		review.error = null;
		review.errorLogPath = null;
	} catch (e) {
		review.items = [];
		review.error = e instanceof Error ? e.message : String(e);
		review.errorLogPath =
			e instanceof ApiError && e.status === 0 ? daemon.logPath || '~/.atlas/atlasd.log' : null;
	} finally {
		review.loading = false;
	}
}

/**
 * Sets one pending memory's status and drops its row. Throws on failure so the
 * caller can toast the daemon's message verbatim; the row stays on failure.
 */
export async function decide(id: Uuid, status: 'active' | 'rejected'): Promise<void> {
	review.busy.push(id);
	try {
		await api().setMemoryStatus(id, status);
		const i = review.items.findIndex((m) => m.id === id);
		if (i >= 0) review.items.splice(i, 1);
	} finally {
		const b = review.busy.indexOf(id);
		if (b >= 0) review.busy.splice(b, 1);
	}
}

/** The pending rows at or above the auto-accept threshold. */
export function qualifying(): Memory[] {
	const min = minConfidence();
	return review.items.filter((m) => m.confidence >= min);
}

export interface BulkResult {
	accepted: number;
	/** The first failure's message, when at least one row could not be accepted. */
	failure: string | null;
	failed: number;
}

/**
 * Accepts every pending row at or above the threshold, one request at a time so a
 * mid-run failure leaves the rest of the list intact and reportable.
 */
export async function acceptAllAboveThreshold(): Promise<BulkResult> {
	const result: BulkResult = { accepted: 0, failure: null, failed: 0 };
	for (const m of qualifying()) {
		try {
			await decide(m.id, 'active');
			result.accepted += 1;
		} catch (e) {
			result.failed += 1;
			result.failure ??= e instanceof Error ? e.message : String(e);
		}
	}
	return result;
}
