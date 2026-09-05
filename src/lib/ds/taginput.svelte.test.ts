// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import TagInput from './TagInput.svelte';

afterEach(cleanup);

describe('TagInput', () => {
	it('adds on Enter or comma, skips repeats, and removes from the badge', async () => {
		const onchange = vi.fn();
		render(TagInput, { value: ['one'], onchange, testId: 'tags' });
		const box = screen.getByTestId('tags') as HTMLInputElement;

		await fireEvent.input(box, { target: { value: 'two' } });
		await fireEvent.keyDown(box, { key: 'Enter' });
		expect(onchange).toHaveBeenLastCalledWith(['one', 'two']);
		expect(box.value).toBe('');

		await fireEvent.input(box, { target: { value: 'one, three' } });
		await fireEvent.keyDown(box, { key: ',' });
		expect(onchange).toHaveBeenLastCalledWith(['one', 'two', 'three']);

		await fireEvent.click(screen.getByTestId('tags-remove-two'));
		expect(onchange).toHaveBeenLastCalledWith(['one', 'three']);

		await fireEvent.keyDown(box, { key: 'Backspace' });
		expect(onchange).toHaveBeenLastCalledWith(['one']);
	});
});
