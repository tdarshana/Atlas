// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import MultiSelect from './MultiSelect.svelte';

afterEach(cleanup);

const options = [
	{ value: 'a', label: 'Alpha' },
	{ value: 'b', label: 'Beta' },
	{ value: 'c', label: 'Gamma' }
];

describe('MultiSelect', () => {
	it('opens on click, filters, and reports the picked values in order', async () => {
		const onchange = vi.fn();
		render(MultiSelect, { options, value: [], label: 'Skills', onchange, testId: 'pick' });
		expect(screen.queryByTestId('pick-search')).toBeNull();

		await fireEvent.click(screen.getByTestId('pick'));
		expect(screen.getByTestId('pick-search')).toBeTruthy();
		expect(screen.getAllByRole('checkbox')).toHaveLength(3);

		await fireEvent.click(screen.getByTestId('pick-b'));
		await fireEvent.click(screen.getByTestId('pick-a'));
		expect(onchange).toHaveBeenLastCalledWith(['b', 'a']);
		expect(screen.getByTestId('pick').textContent).toContain('Beta, Alpha');

		await fireEvent.input(screen.getByTestId('pick-search'), { target: { value: 'gam' } });
		expect(screen.getAllByRole('checkbox')).toHaveLength(1);
		expect(screen.getByTestId('pick-c')).toBeTruthy();
	});

	it('keeps a stored value the options no longer name, and closes on Escape', async () => {
		render(MultiSelect, { options, value: ['zzz'], testId: 'pick' });
		expect(screen.getByTestId('pick').textContent).toContain('zzz');
		await fireEvent.click(screen.getByTestId('pick'));
		expect((screen.getByTestId('pick-zzz') as HTMLInputElement).checked).toBe(true);

		await fireEvent.keyDown(screen.getByTestId('pick-search'), { key: 'Escape' });
		expect(screen.queryByTestId('pick-search')).toBeNull();
	});
});
