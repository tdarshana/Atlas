// The About card's update progress bar: a plain percentage from `atlas:update-progress`'s
// payload, 0 while nothing has downloaded yet or the server sent no content length.

import type { UpdateProgress } from '$lib/types';

export function updateProgressPercent(progress: UpdateProgress | null): number {
	if (!progress?.total) return 0;
	return Math.min(100, Math.round((progress.downloaded / progress.total) * 100));
}
