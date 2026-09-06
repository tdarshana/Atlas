// The board's drag state and the arithmetic behind a drop. One card at a time is
// lifted off its lane and drawn as a ghost under the pointer; the lane under the
// pointer shows a line where the card would land. A drop sends the daemon a position
// between the two cards on either side of that line (or one step past the end), and
// the column sorts by position, so the order holds across clients and restarts.

import type { Task } from '$lib/types';

/** Pointer travel before a press turns into a drag, so a click stays a click. */
export const DRAG_THRESHOLD_PX = 6;

export const drag = $state({
	/** The lifted card, or null when nothing is being dragged. */
	key: null as string | null,
	title: '',
	fromStage: null as string | null,
	/** The lane under the pointer and the insertion index among that lane's other cards. */
	overStage: null as string | null,
	overIndex: -1,
	/** Ghost position, in CSS pixels. */
	x: 0,
	y: 0,
	/** The lifted card's own markup and size, so the ghost is the card, not a stand-in,
	 * and where inside it the pointer took hold. */
	html: '',
	width: 0,
	height: 0,
	grabX: 0,
	grabY: 0
});

/** When the last drop ended, so the click a pointer release also raises is ignored. */
let droppedAt = 0;

export function justDropped(now = Date.now()): boolean {
	return now - droppedAt < 300;
}

export function markDropped(now = Date.now()): void {
	droppedAt = now;
}

export function clearDrag(): void {
	drag.key = null;
	drag.title = '';
	drag.fromStage = null;
	drag.overStage = null;
	drag.overIndex = -1;
	drag.html = '';
	drag.width = 0;
	drag.height = 0;
}

/** A position strictly between two neighbours; past either end steps one further
 * out, and an empty column starts at 1. */
export function positionBetween(prev: number | null, next: number | null): number {
	if (prev === null && next === null) return 1;
	if (prev === null) return (next as number) - 1;
	if (next === null) return prev + 1;
	return (prev + next) / 2;
}

/** The position for dropping `movingKey` at `index` among `tasks` (the column's cards
 * in order); the moving card itself is not a neighbour. */
export function targetPosition(tasks: Task[], index: number, movingKey: string): number {
	const rest = tasks.filter((t) => t.key !== movingKey);
	const i = Math.max(0, Math.min(index, rest.length));
	const prev = i > 0 ? rest[i - 1].position : null;
	const next = i < rest.length ? rest[i].position : null;
	return positionBetween(prev, next);
}

/** True when dropping at `index` in `stage` leaves the card where it already is. */
export function isSameSpot(tasks: Task[], stage: string, index: number, movingKey: string): boolean {
	const moving = tasks.find((t) => t.key === movingKey);
	if (!moving || moving.stage !== stage) return false;
	const rest = tasks.filter((t) => t.key !== movingKey);
	const current = tasks.indexOf(moving);
	return Math.max(0, Math.min(index, rest.length)) === current;
}

/** The insertion index for a pointer at `y` over cards whose vertical middles are
 * `mids` (top to bottom): before the first middle below the pointer. */
export function indexForY(mids: number[], y: number): number {
	for (let i = 0; i < mids.length; i++) if (y < mids[i]) return i;
	return mids.length;
}
