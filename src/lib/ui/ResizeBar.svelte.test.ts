// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render } from '@testing-library/svelte';

import ResizeBar from './ResizeBar.svelte';

afterEach(() => cleanup());

describe('ResizeBar keyboard path', () => {
	it('ArrowRight nudges by 20px, clamped to max', async () => {
		const onresize = vi.fn();
		const { getByTestId } = render(ResizeBar, {
			label: 'Side panel',
			value: 390,
			min: 180,
			max: 400,
			onresize,
			testid: 'rb'
		});

		await fireEvent.keyDown(getByTestId('rb'), { key: 'ArrowRight' });
		expect(onresize).toHaveBeenCalledWith(400);
	});

	it('ArrowLeft nudges by 20px, clamped to min', async () => {
		const onresize = vi.fn();
		const { getByTestId } = render(ResizeBar, {
			label: 'Side panel',
			value: 190,
			min: 180,
			max: 400,
			onresize,
			testid: 'rb'
		});

		await fireEvent.keyDown(getByTestId('rb'), { key: 'ArrowLeft' });
		expect(onresize).toHaveBeenCalledWith(180);
	});

	it('nudges by exactly 20px away from either bound', async () => {
		const onresize = vi.fn();
		const { getByTestId } = render(ResizeBar, {
			label: 'Side panel',
			value: 260,
			min: 180,
			max: 400,
			onresize,
			testid: 'rb'
		});

		await fireEvent.keyDown(getByTestId('rb'), { key: 'ArrowRight' });
		expect(onresize).toHaveBeenCalledWith(280);
	});

	it('ignores every other key', async () => {
		const onresize = vi.fn();
		const { getByTestId } = render(ResizeBar, {
			label: 'Side panel',
			value: 260,
			min: 180,
			max: 400,
			onresize,
			testid: 'rb'
		});

		await fireEvent.keyDown(getByTestId('rb'), { key: 'Enter' });
		expect(onresize).not.toHaveBeenCalled();
	});
});

describe('ResizeBar pointer path under zoom', () => {
	// jsdom does not implement pointer capture; `grab`/`drop` call it unconditionally.
	let setCapture: typeof Element.prototype.setPointerCapture;
	let releaseCapture: typeof Element.prototype.releasePointerCapture;

	beforeEach(() => {
		setCapture = Element.prototype.setPointerCapture;
		releaseCapture = Element.prototype.releasePointerCapture;
		Element.prototype.setPointerCapture = vi.fn();
		Element.prototype.releasePointerCapture = vi.fn();
		document.documentElement.style.setProperty('--ui-zoom', '2');
	});

	afterEach(() => {
		Element.prototype.setPointerCapture = setCapture;
		Element.prototype.releasePointerCapture = releaseCapture;
		document.documentElement.style.removeProperty('--ui-zoom');
	});

	// jsdom has no PointerEvent constructor, so testing-library's `fireEvent.pointerX`
	// helpers fall back to a plain `Event` whose constructor ignores `clientX`/`pointerId`
	// (they are not part of `EventInit`). Dispatching a plain `Event` with those set as
	// own properties gets them to the handler the same way a real PointerEvent would.
	function pointerEvent(type: string, init: { clientX: number; pointerId: number }): Event {
		const event = new Event(type, { bubbles: true, cancelable: true });
		Object.assign(event, init);
		return event;
	}

	it('divides the pointer delta by --ui-zoom so a 100px move grows the width by 50', async () => {
		const onresize = vi.fn();
		const startWidth = 300;
		const { getByTestId } = render(ResizeBar, {
			label: 'Side panel',
			value: startWidth,
			min: 180,
			max: 900,
			onresize,
			testid: 'rb'
		});

		const bar = getByTestId('rb');
		await fireEvent(bar, pointerEvent('pointerdown', { clientX: 0, pointerId: 1 }));
		await fireEvent(bar, pointerEvent('pointermove', { clientX: 100, pointerId: 1 }));
		await fireEvent(bar, pointerEvent('pointerup', { clientX: 100, pointerId: 1 }));

		expect(onresize).toHaveBeenCalledWith(startWidth + 50);
	});
});
