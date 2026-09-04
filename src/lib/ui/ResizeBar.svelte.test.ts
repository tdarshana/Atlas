// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
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
