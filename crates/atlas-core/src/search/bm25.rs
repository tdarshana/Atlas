use std::collections::HashMap;
use uuid::Uuid;
use super::tokenize;

#[derive(Default)]
pub struct Bm25Index {
    docs: HashMap<Uuid, Vec<String>>,
    df: HashMap<String, usize>,
    total_len: usize,
}

impl Bm25Index {
    pub fn new() -> Self { Self::default() }
    pub fn len(&self) -> usize { self.docs.len() }
    pub fn is_empty(&self) -> bool { self.docs.is_empty() }

    pub fn upsert(&mut self, id: Uuid, text: &str) {
        self.remove(id);
        let toks = tokenize(text);
        let mut seen = std::collections::HashSet::new();
        for t in &toks { if seen.insert(t.clone()) { *self.df.entry(t.clone()).or_insert(0) += 1; } }
        self.total_len += toks.len();
        self.docs.insert(id, toks);
    }

    pub fn remove(&mut self, id: Uuid) {
        if let Some(toks) = self.docs.remove(&id) {
            self.total_len -= toks.len();
            let mut seen = std::collections::HashSet::new();
            for t in toks { if seen.insert(t.clone()) { if let Some(n) = self.df.get_mut(&t) { *n -= 1; if *n == 0 { self.df.remove(&t); } } } }
        }
    }

    /// BM25 (k1=1.2, b=0.75), normalized so the best hit scores 1.0.
    pub fn query(&self, q: &str, limit: usize) -> Vec<(Uuid, f64)> {
        let qt = tokenize(q);
        if qt.is_empty() || self.docs.is_empty() { return vec![]; }
        let n = self.docs.len() as f64;
        let avgdl = self.total_len as f64 / n;
        let (k1, b) = (1.2_f64, 0.75_f64);
        let mut scored: Vec<(Uuid, f64)> = self.docs.iter().filter_map(|(id, toks)| {
            let dl = toks.len() as f64;
            let mut s = 0.0;
            for t in &qt {
                let tf = toks.iter().filter(|x| *x == t).count() as f64;
                if tf == 0.0 { continue; }
                let df = *self.df.get(t).unwrap_or(&0) as f64;
                let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                s += idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avgdl));
            }
            (s > 0.0).then_some((*id, s))
        }).collect();
        scored.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
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
}
