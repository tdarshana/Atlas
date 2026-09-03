// Pure helper for the project Practices tab (frame 02.3): splits one `listDocs(kind,
// projectId)` answer, which already carries this project's own docs plus every global
// one, into the two tables the frame draws.

import type { Doc, Uuid } from '$lib/types';

export interface PracticesSplit {
	/** This project's own practices. */
	project: Doc[];
	/** Global practices, inherited by every project. */
	inherited: Doc[];
}

export function splitPractices(docs: Doc[], projectId: Uuid): PracticesSplit {
	return {
		project: docs.filter((d) => d.project_id === projectId),
		inherited: docs.filter((d) => d.project_id === null)
	};
}
