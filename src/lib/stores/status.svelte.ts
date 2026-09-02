// Daemon status, polled while the app is open. The daemon reports `embedding` as
// "ready", "loading" or "unavailable: <reason>" (see Service::embedding_status), so
// the pill shows the word before the colon and keeps the reason for the title.

import { api } from '$lib/daemon.svelte';
import type { StatusReport } from '$lib/types';

export const POLL_MS = 10_000;

export const status = $state({
	report: null as StatusReport | null,
	error: null as string | null,
	loading: false
});

let timer: ReturnType<typeof setInterval> | null = null;

export async function refreshStatus(): Promise<void> {
	status.loading = true;
	try {
		status.report = await api().status();
		status.error = null;
	} catch (e) {
		status.error = e instanceof Error ? e.message : String(e);
	} finally {
		status.loading = false;
	}
}

/** Refreshes now, then every `intervalMs`. Returns the stop function. */
export function startStatusPolling(intervalMs = POLL_MS): () => void {
	stopStatusPolling();
	void refreshStatus();
	timer = setInterval(() => void refreshStatus(), intervalMs);
	return stopStatusPolling;
}

export function stopStatusPolling(): void {
	if (timer !== null) {
		clearInterval(timer);
		timer = null;
	}
}

/** "12 memories · embedding ready", or "daemon offline" once a poll has failed. */
export function statusLabel(): string {
	if (status.error) return 'daemon offline';
	if (!status.report) return 'connecting…';
	const embedding = status.report.embedding.split(':')[0].trim();
	const n = status.report.memories_active;
	return `${n} ${n === 1 ? 'memory' : 'memories'} · embedding ${embedding}`;
}

/** The full embedding string when it carries a reason, otherwise the error. */
export function statusDetail(): string {
	if (status.error) return status.error;
	return status.report?.embedding ?? '';
}
