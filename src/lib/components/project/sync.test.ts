import { describe, expect, it } from 'vitest';
import { chosenSyncKinds, DEFAULT_SYNC_TARGETS, projectSyncRequest } from './sync';

describe('chosenSyncKinds', () => {
	it('maps every target on by default, frame order', () => {
		expect(chosenSyncKinds(DEFAULT_SYNC_TARGETS)).toEqual([
			'claude',
			'codex',
			'agents_md',
			'claude_md'
		]);
	});

	it('drops a target that is off', () => {
		expect(chosenSyncKinds({ ...DEFAULT_SYNC_TARGETS, codex: false, claudeMd: false })).toEqual([
			'claude',
			'agents_md'
		]);
	});

	it('is empty when every target is off', () => {
		expect(
			chosenSyncKinds({ claude: false, codex: false, agentsMd: false, claudeMd: false })
		).toEqual([]);
	});
});

describe('projectSyncRequest', () => {
	it('is rooted at the project and never global', () => {
		const req = projectSyncRequest('/Users/d/atlas', DEFAULT_SYNC_TARGETS, true);
		expect(req.root).toBe('/Users/d/atlas');
		expect(req.global).toBe(false);
		expect(req.check_only).toBe(true);
		expect(req.targets).toEqual(['claude', 'codex', 'agents_md', 'claude_md']);
	});

	it('carries check_only through for a real sync', () => {
		const req = projectSyncRequest('/root', DEFAULT_SYNC_TARGETS, false);
		expect(req.check_only).toBe(false);
	});
});
