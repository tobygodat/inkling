//! A version of the draft, reviewed one change at a time.
//!
//! A [`Review`] owns the draft it was made from and an ordered list of changes. Each change
//! replaces one byte range of the draft with one of its wordings. Nothing touches the draft:
//! [`Review::result`] and [`Review::edits`] say what the writer's decisions add up to.

use similar::DiffTag;
use similar::algorithms::{Capture, myers};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Decision {
    #[default]
    Pending,
    Accepted,
    Rejected,
}

/// Names one change within the review that handed it out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChangeId(usize);

/// Replace `range` of the document with `text`. An empty range inserts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Hunk {
    range: Range<usize>,
    /// Never empty. The AI's first wording comes first.
    wordings: Vec<String>,
    chosen: usize,
    decision: Decision,
}

impl Hunk {
    fn new(range: Range<usize>, wordings: Vec<String>) -> Self {
        debug_assert!(!wordings.is_empty());
        Hunk { range, wordings, chosen: 0, decision: Decision::Pending }
    }

    fn added(&self) -> &str {
        &self.wordings[self.chosen]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Review {
    draft: String,
    /// Ordered by range. Ranges never overlap or touch, and sit on char boundaries of `draft`.
    hunks: Vec<Hunk>,
}

/// One change as a UI draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change<'a> {
    pub id: ChangeId,
    /// The range of the draft this change replaces. Empty for a pure insertion.
    pub range: Range<usize>,
    /// The draft's text in `range`. Empty for a pure insertion.
    pub removed: &'a str,
    /// The chosen wording. Empty for a pure deletion.
    pub added: &'a str,
    pub decision: Decision,
    /// Every wording, the AI's first.
    pub wordings: &'a [String],
    /// Index of `added` in `wordings`.
    pub chosen: usize,
    /// How many wordings there are besides the chosen one, for "n more".
    pub more: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment<'a> {
    Unchanged(&'a str),
    Change(Change<'a>),
}

/// The numbers on the version bar. Word counts are over every change's chosen wording,
/// whatever its decision, so they describe the version, not the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Stats {
    pub words_added: usize,
    pub words_removed: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub pending: usize,
    pub total: usize,
}

impl Review {
    /// The changes that turn `draft` into `version`, by a word-level diff. Changes that touch,
    /// or that only spaces or tabs separate, are one change. A line break keeps them apart.
    pub fn new(draft: impl Into<String>, version: &str) -> Self {
        let draft = draft.into();
        let hunks = diff(&draft, version);
        Review { draft, hunks }
    }

    /// One change at `range` with several wordings to pick from. `None` when the range is not
    /// a char-boundary range of `draft` or there are no wordings.
    pub fn alternatives(draft: impl Into<String>, range: Range<usize>, wordings: Vec<String>) -> Option<Self> {
        let draft = draft.into();
        if wordings.is_empty() || !is_range_of(&draft, &range) {
            return None;
        }
        Some(Review { draft, hunks: vec![Hunk::new(range, wordings)] })
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }

    pub fn is_empty(&self) -> bool {
        self.hunks.is_empty()
    }

    pub fn changes(&self) -> impl Iterator<Item = Change<'_>> {
        (0..self.hunks.len()).map(|i| self.view(i))
    }

    pub fn change(&self, id: ChangeId) -> Option<Change<'_>> {
        (id.0 < self.hunks.len()).then(|| self.view(id.0))
    }

    /// The first change still pending at or after `id`, wrapping around. For "next change".
    pub fn next_pending(&self, after: Option<ChangeId>) -> Option<ChangeId> {
        let n = self.hunks.len();
        let start = after.map_or(0, |id| id.0 + 1);
        (0..n).map(|k| (start + k) % n).find(|&i| self.hunks[i].decision == Decision::Pending).map(ChangeId)
    }

    /// The draft, cut into unchanged text and changes, in order.
    pub fn segments(&self) -> Vec<Segment<'_>> {
        let mut out = Vec::with_capacity(self.hunks.len() * 2 + 1);
        let mut at = 0;
        for (i, hunk) in self.hunks.iter().enumerate() {
            if hunk.range.start > at {
                out.push(Segment::Unchanged(&self.draft[at..hunk.range.start]));
            }
            out.push(Segment::Change(self.view(i)));
            at = hunk.range.end;
        }
        if at < self.draft.len() {
            out.push(Segment::Unchanged(&self.draft[at..]));
        }
        out
    }

    pub fn stats(&self) -> Stats {
        let mut stats = Stats { total: self.hunks.len(), ..Stats::default() };
        for hunk in &self.hunks {
            stats.words_added += hunk.added().unicode_words().count();
            stats.words_removed += self.draft[hunk.range.clone()].unicode_words().count();
            match hunk.decision {
                Decision::Pending => stats.pending += 1,
                Decision::Accepted => stats.accepted += 1,
                Decision::Rejected => stats.rejected += 1,
            }
        }
        stats
    }

    pub fn accept(&mut self, id: ChangeId) {
        self.decide(id, Decision::Accepted);
    }

    pub fn reject(&mut self, id: ChangeId) {
        self.decide(id, Decision::Rejected);
    }

    /// Takes back a decision: the change is pending again.
    pub fn undo(&mut self, id: ChangeId) {
        self.decide(id, Decision::Pending);
    }

    /// Every pending change becomes accepted. Rejected changes stay rejected.
    pub fn accept_rest(&mut self) {
        for hunk in self.hunks.iter_mut().filter(|h| h.decision == Decision::Pending) {
            hunk.decision = Decision::Accepted;
        }
    }

    /// Uses another wording for a change. Its decision is unchanged. Out-of-range is ignored.
    pub fn choose(&mut self, id: ChangeId, wording: usize) {
        if let Some(hunk) = self.hunks.get_mut(id.0).filter(|h| wording < h.wordings.len()) {
            hunk.chosen = wording;
        }
    }

    /// The draft with every accepted change applied.
    pub fn result(&self) -> String {
        let mut out = String::with_capacity(self.draft.len());
        let mut at = 0;
        for hunk in self.hunks.iter().filter(|h| h.decision == Decision::Accepted) {
            out.push_str(&self.draft[at..hunk.range.start]);
            out.push_str(hunk.added());
            at = hunk.range.end;
        }
        out.push_str(&self.draft[at..]);
        out
    }

    /// Each accepted change as an edit to the draft, last first. Applying them in this order
    /// to the draft gives [`Review::result`]: every range is still valid when its turn comes.
    pub fn edits(&self) -> Vec<Edit> {
        self.hunks
            .iter()
            .rev()
            .filter(|h| h.decision == Decision::Accepted)
            .map(|h| Edit { range: h.range.clone(), text: h.added().to_string() })
            .collect()
    }

    fn decide(&mut self, id: ChangeId, decision: Decision) {
        if let Some(hunk) = self.hunks.get_mut(id.0) {
            hunk.decision = decision;
        }
    }

    fn view(&self, i: usize) -> Change<'_> {
        let hunk = &self.hunks[i];
        Change {
            id: ChangeId(i),
            range: hunk.range.clone(),
            removed: &self.draft[hunk.range.clone()],
            added: hunk.added(),
            decision: hunk.decision,
            wordings: &hunk.wordings,
            chosen: hunk.chosen,
            more: hunk.wordings.len() - 1,
        }
    }
}

pub(crate) fn is_range_of(text: &str, range: &Range<usize>) -> bool {
    range.start <= range.end
        && range.end <= text.len()
        && text.is_char_boundary(range.start)
        && text.is_char_boundary(range.end)
}

/// A run of the diff in byte ranges: equal text, or a change.
struct Run {
    equal: bool,
    old: Range<usize>,
    new: Range<usize>,
}

fn diff(old: &str, new: &str) -> Vec<Hunk> {
    let old_tokens: Vec<&str> = old.split_word_bounds().collect();
    let new_tokens: Vec<&str> = new.split_word_bounds().collect();
    let old_at = offsets(&old_tokens);
    let new_at = offsets(&new_tokens);
    // similar's text API runs a compaction pass that can emit out-of-order ops (2.7.0), so
    // this calls Myers directly with a plain capture.
    let mut capture = Capture::new();
    myers::diff(&mut capture, &old_tokens, 0..old_tokens.len(), &new_tokens, 0..new_tokens.len())
        .expect("capturing a diff never fails");

    let mut runs: Vec<Run> = Vec::new();
    for op in capture.ops() {
        let (tag, o, n) = op.as_tag_tuple();
        let run = Run { equal: tag == DiffTag::Equal, old: old_at[o.start]..old_at[o.end], new: new_at[n.start]..new_at[n.end] };
        match runs.last_mut() {
            Some(last) if last.equal == run.equal => {
                last.old.end = run.old.end;
                last.new.end = run.new.end;
            }
            _ => runs.push(run),
        }
    }

    // A gap of spaces between two changes joins them: "quick brown" to "slow red" is one edit.
    let joins = |run: &Run| run.equal && old[run.old.clone()].chars().all(|c| c == ' ' || c == '\t');
    let mut merged: Vec<Run> = Vec::new();
    for (i, run) in runs.iter().enumerate() {
        let between_changes = i > 0 && i + 1 < runs.len();
        let absorb = !run.equal || (between_changes && joins(run));
        match merged.last_mut() {
            Some(last) if absorb && !last.equal => {
                last.old.end = run.old.end;
                last.new.end = run.new.end;
            }
            _ => merged.push(Run { equal: run.equal, old: run.old.clone(), new: run.new.clone() }),
        }
    }

    merged
        .into_iter()
        .filter(|run| !run.equal)
        .map(|run| Hunk::new(run.old, vec![new[run.new].to_string()]))
        .collect()
}

/// Byte offset of each token's start, plus the end.
fn offsets(tokens: &[&str]) -> Vec<usize> {
    let mut at = Vec::with_capacity(tokens.len() + 1);
    let mut sum = 0;
    at.push(0);
    for token in tokens {
        sum += token.len();
        at.push(sum);
    }
    at
}

#[cfg(test)]
mod tests;
