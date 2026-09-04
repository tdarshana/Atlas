import { describe, expect, it } from 'vitest';
import { agentAccessSummary } from './mcp';
import type { AgentAccess } from '$lib/types';

describe('agentAccessSummary', () => {
	it('reads a null list as any actor', () => {
		const access: AgentAccess = { memory_writers: null, task_movers: null, require_review: false };
		expect(agentAccessSummary(access)).toEqual({
			memoryWriters: 'Any actor',
			taskMovers: 'Any actor',
			requireReview: false
		});
	});

	it('joins a set list, and passes require_review through', () => {
		const access: AgentAccess = {
			memory_writers: ['claude-code', 'codex'],
			task_movers: ['claude-code/reviewer'],
			require_review: true
		};
		expect(agentAccessSummary(access)).toEqual({
			memoryWriters: 'claude-code, codex',
			taskMovers: 'claude-code/reviewer',
			requireReview: true
		});
	});
});
