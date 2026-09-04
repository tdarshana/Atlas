// Scrolling one section of the Atlas detail into view without touching anything else on
// the page. `Element.scrollIntoView` scrolls *every* scrollable ancestor of its target,
// so a jump inside the docked detail panel also dragged the table's own scroll region,
// taking the page title, the summary line and the table header out of the frame. These
// helpers move the detail's own scroll container and nothing else.

/** The nearest ancestor of `node` that actually scrolls vertically, or null. */
export function scrollParent(node: HTMLElement): HTMLElement | null {
	let current = node.parentElement;
	while (current) {
		const overflowY = getComputedStyle(current).overflowY;
		if (
			(overflowY === 'auto' || overflowY === 'scroll') &&
			current.scrollHeight > current.clientHeight
		) {
			return current;
		}
		current = current.parentElement;
	}
	return null;
}

/**
 * Puts the section with `id` at the top of the detail's own scroll container. `root` is
 * the component's own element, so the lookup is scoped to this detail rather than to the
 * document: the global view and the project tab both name their sections `tools`,
 * `resources` and `clients`. A missing section, or a detail that does not scroll, is a
 * no-op rather than a jump to somewhere arbitrary.
 */
export function scrollDetailTo(root: HTMLElement, id: string): void {
	// An attribute selector rather than `#id`: the section names are ours, but this way a
	// name needing escaping simply fails to match instead of throwing a selector error,
	// and it does not depend on `CSS.escape`, which jsdom does not carry.
	if (!/^[A-Za-z0-9_-]+$/.test(id)) return;
	const target = root.querySelector<HTMLElement>(`[id="${id}"]`);
	if (!target) return;

	const container = scrollParent(target);
	if (!container) return;

	const delta = target.getBoundingClientRect().top - container.getBoundingClientRect().top;
	container.scrollTop += delta;
}
