// One place to turn an unknown thrown value into something a user can read, so
// every screen reports a failure the same way.

import { ApiError } from './api';
import { logPath } from './format';

/**
 * The message to show. `ApiError` extends `Error` and its message is already the
 * daemon's `error` string verbatim, so one branch covers both.
 */
export function errorMessage(e: unknown): string {
	if (e instanceof Error) return e.message;
	return String(e);
}

/**
 * The daemon log path when the failure was a transport failure (`ApiError` with
 * status 0, the one case where the log explains it), otherwise null.
 */
export function errorLogPath(e: unknown): string | null {
	return e instanceof ApiError && e.status === 0 ? logPath() : null;
}
