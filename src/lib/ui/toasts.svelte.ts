// A tiny toast queue. `push` returns the id so a caller can dismiss early; every
// toast auto-dismisses after `AUTO_DISMISS_MS`.

export type ToastKind = 'info' | 'success' | 'error';

export interface ToastItem {
	id: number;
	kind: ToastKind;
	text: string;
}

export const AUTO_DISMISS_MS = 4000;

export const toasts = $state<ToastItem[]>([]);

let nextId = 0;

export function push(kind: ToastKind, text: string, ttlMs = AUTO_DISMISS_MS): number {
	const id = ++nextId;
	toasts.push({ id, kind, text });
	if (ttlMs > 0) setTimeout(() => dismiss(id), ttlMs);
	return id;
}

export function dismiss(id: number): void {
	const i = toasts.findIndex((t) => t.id === id);
	if (i >= 0) toasts.splice(i, 1);
}

export function clear(): void {
	toasts.length = 0;
}
