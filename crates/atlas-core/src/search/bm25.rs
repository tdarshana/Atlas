use std::collections::HashMap;
use uuid::Uuid;
use super::tokenize;

/// One indexed document: its token count and the ids of the terms it contains.
struct Doc { len: u32, terms: Vec<u32> }

/// One entry of a term's posting list: a document, the term's frequency in it and
/// the document's length, so scoring never has to look the document up.
#[derive(Clone, Copy)]
struct Posting { id: Uuid, tf: u32, len: u32 }

/// An inverted BM25 index. Terms are interned once, so a document costs one
/// `Vec<u32>` rather than one heap `String` per token, and a query touches only
/// the postings of its own terms rather than every document. Postings are dense
/// vectors: removal is a linear scan of each of the document's terms, which is
/// cheap next to the build and the queries it trades for.
#[derive(Default)]
pub struct Bm25Index {
    /// Term text to its id. A term keeps its id for the life of the index.
    terms: HashMap<String, u32>,
    /// Per term id, the documents containing it.
    postings: Vec<Vec<Posting>>,
    docs: HashMap<Uuid, Doc>,
    total_len: usize,
}

impl Bm25Index {
    pub fn new() -> Self { Self::default() }
    pub fn len(&self) -> usize { self.docs.len() }
    pub fn is_empty(&self) -> bool { self.docs.is_empty() }

    pub fn upsert(&mut self, id: Uuid, text: &str) {
        self.remove(id);
        let toks = tokenize(text);
        let len = toks.len() as u32;
        let mut ids: Vec<u32> = toks.into_iter().map(|t| match self.terms.get(&t) {
            Some(&term) => term,
            None => {
                let term = self.postings.len() as u32;
                self.terms.insert(t, term);
                self.postings.push(Vec::new());
                term
            }
        }).collect();
        // Sorted, equal ids are adjacent, so one pass counts each term's frequency.
        ids.sort_unstable();
        let mut terms = Vec::new();
        let mut i = 0;
        while i < ids.len() {
            let term = ids[i];
            let mut tf = 0;
            while i < ids.len() && ids[i] == term { tf += 1; i += 1; }
            self.postings[term as usize].push(Posting { id, tf, len });
            terms.push(term);
        }
        self.total_len += len as usize;
        self.docs.insert(id, Doc { len, terms });
    }

    pub fn remove(&mut self, id: Uuid) {
        if let Some(doc) = self.docs.remove(&id) {
            self.total_len -= doc.len as usize;
            for term in doc.terms {
                let posting = &mut self.postings[term as usize];
                if let Some(i) = posting.iter().position(|p| p.id == id) { posting.swap_remove(i); }
            }
        }
    }

    /// BM25 (k1=1.2, b=0.75), normalized so the best hit scores 1.0.
    pub fn query(&self, q: &str, limit: usize) -> Vec<(Uuid, f64)> {
        let qt = tokenize(q);
        if qt.is_empty() || self.docs.is_empty() { return vec![]; }
        let n = self.docs.len() as f64;
        let avgdl = self.total_len as f64 / n;
        let (k1, b) = (1.2_f64, 0.75_f64);
        let mut scores: HashMap<Uuid, f64> = HashMap::new();
        for t in &qt {
            let Some(&term) = self.terms.get(t) else { continue };
            let posting = &self.postings[term as usize];
            if posting.is_empty() { continue; }
            let df = posting.len() as f64;
            let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
            for p in posting {
                let (tf, dl) = (p.tf as f64, p.len as f64);
                *scores.entry(p.id).or_insert(0.0) += idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avgdl));
            }
        }
        let mut scored: Vec<(Uuid, f64)> = scores.into_iter().filter(|(_, s)| *s > 0.0).collect();
        scored.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| x.0.cmp(&y.0)));
        scored.truncate(limit);
        if let Some(top) = scored.first().map(|h| h.1) { for h in &mut scored { h.1 /= top; } }
        scored
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    #[test]
    fn ranks_matching_doc_first_and_normalizes() {
        let mut idx = Bm25Index::new();
        let a = Uuid::new_v4(); let b = Uuid::new_v4(); let c = Uuid::new_v4();
        idx.upsert(a, "the project uses bun as package manager");
        idx.upsert(b, "tailwind theme lives in globals.css");
        idx.upsert(c, "convex runs in docker on port 3210");
        let hits = idx.query("which package manager, bun or pnpm", 10);
        assert_eq!(hits[0].0, a);
        assert!((hits[0].1 - 1.0).abs() < 1e-9);
        assert!(hits.iter().all(|h| h.1 <= 1.0 && h.1 > 0.0));
        idx.remove(a);
        assert!(idx.query("bun", 10).is_empty());
    }

    /// Upserting a document again replaces its terms rather than adding to them,
    /// and a repeated term counts by frequency, not once per occurrence in the text.
    #[test]
    fn upsert_replaces_and_counts_term_frequency() {
        let mut idx = Bm25Index::new();
        let a = Uuid::new_v4(); let b = Uuid::new_v4();
        idx.upsert(a, "redis redis redis cache");
        idx.upsert(b, "redis cache");
        let hits = idx.query("redis", 10);
        assert_eq!(hits[0].0, a, "the document repeating the term ranks first");
        assert_eq!(hits.len(), 2);
        idx.upsert(a, "postgres cache");
        assert_eq!(idx.query("redis", 10).len(), 1);
        assert_eq!(idx.query("postgres", 10)[0].0, a);
        assert_eq!(idx.len(), 2);
    }
}
