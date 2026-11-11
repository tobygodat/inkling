use super::*;
use crate::ai::{Backend, start};
use crate::review::{Decision, Segment};
use std::sync::mpsc;
use std::time::Duration;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn find_a_word_reads_a_json_list() {
    assert_eq!(parse_alternatives(r#"["hushed", "still"]"#, "quiet"), Ok(words(&["hushed", "still"])));
}

#[test]
fn find_a_word_reads_a_list_inside_a_fence_or_words() {
    let fenced = "```json\n[\"hushed\", \"still\"]\n```";
    assert_eq!(parse_alternatives(fenced, "quiet"), Ok(words(&["hushed", "still"])));
    let chatty = "Here are some options: [\"hushed\", \"still\"] Hope that helps!";
    assert_eq!(parse_alternatives(chatty, "quiet"), Ok(words(&["hushed", "still"])));
}

#[test]
fn find_a_word_drops_blanks_repeats_and_the_original() {
    let answer = r#"[" hushed ", "", "quiet", "hushed", "still"]"#;
    assert_eq!(parse_alternatives(answer, "quiet"), Ok(words(&["hushed", "still"])));
}

#[test]
fn a_malformed_find_a_word_answer_fails_cleanly() {
    assert_eq!(parse_alternatives("hushed, still, silent", "quiet"), Err(AnswerError::NotAList));
    assert_eq!(parse_alternatives(r#"["hushed", "still""#, "quiet"), Err(AnswerError::NotAList));
    assert_eq!(parse_alternatives("] backwards [", "quiet"), Err(AnswerError::NotAList));
    assert_eq!(parse_alternatives(r#"[1, 2]"#, "quiet"), Err(AnswerError::NotAList));
    assert_eq!(parse_alternatives(r#"{"words": "hushed"}"#, "quiet"), Err(AnswerError::NotAList));
    assert_eq!(parse_alternatives("[]", "quiet"), Err(AnswerError::NoAlternatives));
    assert_eq!(parse_alternatives(r#"["quiet"]"#, "quiet"), Err(AnswerError::NoAlternatives));
}

#[test]
fn find_a_word_becomes_one_change_with_every_wording() {
    let draft = "The house was quiet tonight.";
    let request = Request::new(draft, Ask::FindWord(13..20)).unwrap();
    assert_eq!(request.ask(), &Ask::FindWord(14..19), "the selection is trimmed to its word");
    let review = request.review(r#"["hushed", "still"]"#).unwrap();
    let change = review.changes().next().unwrap();
    assert_eq!((change.removed, change.added, change.more), ("quiet", "hushed", 1));
    assert_eq!(review.changes().count(), 1);
}

#[test]
fn requests_are_checked_against_their_draft() {
    assert_eq!(Request::new("café", Ask::NextLine { at: 4 }).unwrap_err(), BadRequest::OutOfRange);
    assert_eq!(Request::new("café", Ask::FindWord(0..9)).unwrap_err(), BadRequest::OutOfRange);
    assert_eq!(Request::new("a  b", Ask::FindWord(1..3)).unwrap_err(), BadRequest::EmptySelection);
    assert_eq!(Request::new("a", Ask::Freeform("  ".into())).unwrap_err(), BadRequest::EmptyInstruction);
    assert!(Request::new("café", Ask::NextLine { at: 5 }).is_ok());
}

#[test]
fn every_prompt_says_what_to_send_back() {
    let draft = "The house was quiet.";
    let whole = [Ask::FixGrammar, Ask::Freeform("Make it warmer".into())];
    for ask in whole {
        let p = Request::new(draft, ask).unwrap().prompt();
        assert!(p.user.contains("<draft>\nThe house was quiet.\n</draft>") && p.user.contains("whole revised draft"));
    }
    let next = Request::new(draft, Ask::NextLine { at: 9 }).unwrap().prompt();
    assert!(next.user.contains("<before_cursor>\nThe house\n</before_cursor>"));
    let find = Request::new(draft, Ask::FindWord(14..19)).unwrap().prompt();
    assert!(find.user.contains("<selected>\nquiet\n</selected>") && find.user.contains("JSON array"));
}

#[test]
fn a_whole_draft_answer_keeps_the_drafts_own_ends() {
    let request = Request::new("\nthe cat sat\n\n", Ask::FixGrammar).unwrap();
    let review = request.review("```\nThe cat sat.\n```").unwrap();
    let changed: Vec<_> = review.changes().map(|c| (c.removed, c.added)).collect();
    assert_eq!(changed, [("the", "The"), ("", ".")]);
}

#[test]
fn next_line_is_one_insertion_with_the_space_it_needs() {
    let request = Request::new("Once upon a time", Ask::NextLine { at: 16 }).unwrap();
    let mut review = request.review("there was a kettle.\n").unwrap();
    assert_eq!(review.changes().count(), 1);
    review.accept_rest();
    assert_eq!(review.result(), "Once upon a time there was a kettle.");

    let cjk = Request::new("我喜欢猫。", Ask::NextLine { at: 15 }).unwrap();
    let mut review = cjk.review("它很可爱。").unwrap();
    review.accept_rest();
    assert_eq!(review.result(), "我喜欢猫。它很可爱。");
}

#[test]
fn an_empty_answer_fails() {
    let request = Request::new("x", Ask::FixGrammar).unwrap();
    assert_eq!(request.review("  \n"), Err(AnswerError::Empty));
}

#[test]
fn a_version_is_complete_only_at_done() {
    let mut revision = Revision::asking(Request::new("a big dog", Ask::Freeform("tighten".into())).unwrap());
    revision.apply(AiEvent::Delta("a vast ".into()));
    assert!(revision.is_busy() && revision.review().is_none());
    revision.apply(AiEvent::Delta("dog".into()));
    revision.apply(AiEvent::Done);
    assert_eq!(revision.review().unwrap().changes().count(), 1);
    revision.apply(AiEvent::Delta("late".into()));
    assert!(revision.review().is_some());
}

#[test]
fn a_failed_or_unparsable_answer_is_a_failure_to_show() {
    let mut revision = Revision::asking(Request::new("quiet", Ask::FindWord(0..5)).unwrap());
    revision.apply(AiEvent::Delta("hushed or still".into()));
    revision.apply(AiEvent::Done);
    assert_eq!(revision, Revision::Failed(AnswerError::NotAList.to_string()));

    let mut revision = Revision::asking(Request::new("x", Ask::FixGrammar).unwrap());
    revision.apply(AiEvent::Failed("offline".into()));
    assert_eq!(revision, Revision::Failed("offline".into()));
}

/// Streams a request's canned answer through `start`, the way a headless UI run does.
fn run_fake(request: Request) -> Revision {
    let (tx, rx) = mpsc::channel();
    let backend = Backend::Fake { text: request.fake_answer(), word_delay: Duration::ZERO };
    start(&request, backend, move |e| tx.send(e).unwrap());
    let mut revision = Revision::asking(request);
    for event in rx {
        revision.apply(event);
    }
    revision
}

#[test]
fn the_fake_backend_gives_a_review_for_every_quick_action() {
    let draft = "i dont think the big house was very quiet that night.\n\nShe walked  to teh door";
    let asks = [
        Ask::FixGrammar,
        Ask::Freeform("Make it warmer".into()),
        Ask::NextLine { at: draft.len() },
        Ask::FindWord(39..44),
    ];
    for ask in asks {
        let revision = run_fake(Request::new(draft, ask.clone()).unwrap());
        let review = revision.review().unwrap_or_else(|| panic!("{ask:?} gave {revision:?}"));
        assert!(!review.is_empty(), "{ask:?} gave no changes");
    }
}

#[test]
fn a_review_runs_from_ask_to_edits() {
    let draft = "i dont think the house was quiet.\nShe walked  to teh door";
    let mut revision = run_fake(Request::new(draft, Ask::FixGrammar).unwrap());
    let review = revision.review_mut().unwrap();
    let shown: Vec<_> = review
        .segments()
        .into_iter()
        .filter_map(|s| match s {
            Segment::Change(c) => Some((c.removed, c.added)),
            Segment::Unchanged(_) => None,
        })
        .collect();
    assert_eq!(shown, [("i dont", "I don't"), ("  ", " "), ("teh", "the"), ("", ".")]);

    let ids: Vec<_> = review.changes().map(|c| c.id).collect();
    review.reject(ids[0]);
    review.accept_rest();
    assert_eq!(review.change(ids[1]).unwrap().decision, Decision::Accepted);
    let expected = "i dont think the house was quiet.\nShe walked to the door.";
    assert_eq!(review.result(), expected);

    let edits = revision.finish();
    assert_eq!(revision, Revision::Idle);
    let mut doc = draft.to_string();
    for edit in &edits {
        doc.replace_range(edit.range.clone(), &edit.text);
    }
    assert_eq!(doc, expected);
}
