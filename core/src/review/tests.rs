use super::*;
use proptest::prelude::*;

const VOCAB: &[&str] = &[
    "the", "The", "work", "of", "moving", "quick", "brown", "fox", "a", "kettle", "naïve", "café", "😀", "👩‍👩‍👧",
    "🇬🇧", "猫", "我", "喜欢", "東京", "。", ",", ".", "!", "'", "\"", "-", " ", " ", " ", " ", "  ", "\t", "\n", "\n\n",
];

fn text() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(VOCAB), 0..40).prop_map(|t| t.concat())
}

/// A version is usually the draft with some tokens swapped, dropped or added, sometimes unrelated.
fn draft_and_version() -> impl Strategy<Value = (String, String)> {
    let edits = prop::collection::vec((0..4u8, prop::sample::select(VOCAB)), 0..40);
    let tokens = prop::collection::vec(prop::sample::select(VOCAB), 0..40);
    prop_oneof![
        3 => (tokens, edits).prop_map(|(tokens, edits)| {
            let draft = tokens.concat();
            let mut version = String::new();
            for (i, token) in tokens.iter().enumerate() {
                match edits.get(i) {
                    Some((0, other)) => version.push_str(other),
                    Some((1, _)) => {}
                    Some((2, other)) => {
                        version.push_str(token);
                        version.push_str(other);
                    }
                    _ => version.push_str(token),
                }
            }
            (draft, version)
        }),
        1 => (text(), text()),
    ]
}

fn decide_all(review: &mut Review, decisions: &[u8]) {
    let ids: Vec<ChangeId> = review.changes().map(|c| c.id).collect();
    for (id, d) in ids.into_iter().zip(decisions.iter().cycle()) {
        match d % 3 {
            0 => review.accept(id),
            1 => review.reject(id),
            _ => {}
        }
    }
}

fn apply(draft: &str, edits: &[Edit]) -> String {
    let mut doc = draft.to_string();
    for edit in edits {
        doc.replace_range(edit.range.clone(), &edit.text);
    }
    doc
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn accepting_every_change_gives_the_version((draft, version) in draft_and_version()) {
        let mut review = Review::new(draft, &version);
        review.accept_rest();
        prop_assert_eq!(review.result(), version.clone());
        prop_assert_eq!(apply(review.draft(), &review.edits()), version);
    }

    #[test]
    fn rejecting_everything_or_deciding_nothing_gives_the_draft((draft, version) in draft_and_version()) {
        let mut review = Review::new(draft.clone(), &version);
        prop_assert_eq!(review.result(), draft.clone());
        prop_assert!(review.edits().is_empty());
        let ids: Vec<ChangeId> = review.changes().map(|c| c.id).collect();
        for id in ids {
            review.reject(id);
        }
        review.accept_rest();
        prop_assert_eq!(review.result(), draft);
    }

    #[test]
    fn segments_rebuild_the_draft_and_the_version((draft, version) in draft_and_version()) {
        let review = Review::new(draft.clone(), &version);
        let (mut old, mut new) = (String::new(), String::new());
        for segment in review.segments() {
            match segment {
                Segment::Unchanged(text) => {
                    prop_assert!(!text.is_empty());
                    old.push_str(text);
                    new.push_str(text);
                }
                Segment::Change(change) => {
                    prop_assert!(!(change.removed.is_empty() && change.added.is_empty()));
                    old.push_str(change.removed);
                    new.push_str(change.added);
                }
            }
        }
        prop_assert_eq!(old, draft);
        prop_assert_eq!(new, version);
    }

    #[test]
    fn ranges_are_ordered_apart_and_on_char_boundaries((draft, version) in draft_and_version()) {
        let review = Review::new(draft.clone(), &version);
        let mut last_end: Option<usize> = None;
        for change in review.changes() {
            let r = change.range.clone();
            prop_assert!(r.start <= r.end && r.end <= draft.len());
            prop_assert!(draft.is_char_boundary(r.start) && draft.is_char_boundary(r.end));
            if let Some(end) = last_end {
                prop_assert!(end < r.start, "changes touch or overlap at {end} and {r:?}");
            }
            last_end = Some(r.end);
        }
    }

    #[test]
    fn any_decisions_give_the_matching_text_and_edits(
        (draft, version) in draft_and_version(),
        decisions in prop::collection::vec(any::<u8>(), 1..8),
    ) {
        let mut review = Review::new(draft, &version);
        decide_all(&mut review, &decisions);
        let expected: String = review
            .segments()
            .iter()
            .map(|s| match s {
                Segment::Unchanged(t) => *t,
                Segment::Change(c) if c.decision == Decision::Accepted => c.added,
                Segment::Change(c) => c.removed,
            })
            .collect();
        prop_assert_eq!(review.result(), expected.clone());
        prop_assert_eq!(apply(review.draft(), &review.edits()), expected);
        let stats = review.stats();
        prop_assert_eq!(stats.accepted + stats.rejected + stats.pending, stats.total);
    }
}

fn only_change(draft: &str, version: &str) -> (String, String) {
    let review = Review::new(draft, version);
    let changes: Vec<_> = review.changes().map(|c| (c.removed.to_string(), c.added.to_string())).collect();
    assert_eq!(changes.len(), 1, "{changes:?}");
    changes.into_iter().next().unwrap()
}

#[test]
fn dropping_words_is_one_change() {
    let (removed, added) = only_change("It was the work of moving house.", "It was the moving house.");
    assert_eq!((removed.trim(), added.trim()), ("work of", ""));
}

#[test]
fn neighbouring_replacements_are_one_change() {
    assert_eq!(only_change("the quick brown fox", "the slow red fox"), ("quick brown".into(), "slow red".into()));
}

#[test]
fn a_replaced_run_of_cjk_is_one_change() {
    assert_eq!(only_change("我喜欢猫。", "我讨厌狗。"), ("喜欢猫".into(), "讨厌狗".into()));
}

#[test]
fn a_whitespace_change_belongs_to_the_word_change_beside_it() {
    assert_eq!(only_change("one  two three", "one deux three"), ("  two".into(), " deux".into()));
}

#[test]
fn separate_edits_stay_separate() {
    let review = Review::new("The cat sat on the mat.", "A cat sat on the rug.");
    let changes: Vec<_> = review.changes().map(|c| (c.removed, c.added)).collect();
    assert_eq!(changes, [("The", "A"), ("mat", "rug")]);
}

#[test]
fn a_line_break_keeps_changes_apart() {
    let review = Review::new("one\ntwo", "uno\ndos");
    assert_eq!(review.changes().count(), 2);
}

#[test]
fn an_unchanged_version_has_no_changes() {
    let review = Review::new("Same text.\n", "Same text.\n");
    assert!(review.is_empty());
    assert_eq!(review.segments(), [Segment::Unchanged("Same text.\n")]);
    assert_eq!(review.stats(), Stats::default());
}

#[test]
fn stats_count_words_and_decisions() {
    let mut review = Review::new("The cat sat on the mat.", "A big cat sat on the rug.");
    let first = review.changes().next().unwrap().id;
    review.accept(first);
    let stats = review.stats();
    assert_eq!((stats.words_added, stats.words_removed), (3, 2));
    assert_eq!((stats.accepted, stats.pending, stats.total), (1, 1, 2));
}

#[test]
fn choosing_another_wording_changes_what_accepting_writes() {
    let draft = "The house was quiet.";
    let words = vec!["hushed".to_string(), "still".to_string(), "silent".to_string()];
    let mut review = Review::alternatives(draft, 14..19, words).unwrap();
    let id = review.changes().next().unwrap().id;
    let change = review.change(id).unwrap();
    assert_eq!((change.removed, change.added, change.more), ("quiet", "hushed", 2));

    review.choose(id, 2);
    review.choose(id, 9);
    assert_eq!(review.change(id).unwrap().added, "silent");
    assert_eq!(review.change(id).unwrap().decision, Decision::Pending);
    assert_eq!(review.result(), draft);

    review.accept(id);
    assert_eq!(review.result(), "The house was silent.");
    assert_eq!(review.edits(), [Edit { range: 14..19, text: "silent".into() }]);
}

#[test]
fn undo_returns_a_change_to_pending_and_accept_the_rest_skips_rejected() {
    let mut review = Review::new("a b c", "x b y");
    let ids: Vec<_> = review.changes().map(|c| c.id).collect();
    review.reject(ids[0]);
    review.accept(ids[1]);
    review.undo(ids[1]);
    assert_eq!(review.change(ids[1]).unwrap().decision, Decision::Pending);
    assert_eq!(review.next_pending(None), Some(ids[1]));
    review.accept_rest();
    assert_eq!(review.result(), "a b y");
    assert_eq!(review.next_pending(None), None);
}

#[test]
fn alternatives_refuse_a_range_inside_a_character() {
    assert!(Review::alternatives("café", 3..4, vec!["x".into()]).is_none());
    assert!(Review::alternatives("café", 0..3, vec![]).is_none());
}

