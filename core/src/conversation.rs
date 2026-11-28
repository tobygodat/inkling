//! The Ask bar's one conversation (TOB-559): every turn, the screen it was asked from and what came
//! back, kept in `conversation.json` in the data folder so it carries across tabs and launches.

use crate::store::Paths;
use serde::{Deserialize, Serialize};
use std::{fs, io};

/// The most turns kept. Older ones go first.
pub const KEPT: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub asked: String,
    /// Where it was asked: "Walden · Economy", "On Keeping a Quiet Desk", "Inbox".
    pub context: String,
    pub answer: String,
    #[serde(default)]
    pub citations: Vec<Citation>,
    /// An edit the inline review took over: its answer says what became of it.
    #[serde(default)]
    pub edit: bool,
    /// Milliseconds since the Unix epoch.
    pub made: u64,
}

/// A passage the answer cites by its number: where it is, as a highlight addresses one (ADR 0006).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Citation {
    pub n: u32,
    pub item: String,
    pub title: String,
    /// The chapter's title, for a book of more than one.
    #[serde(default)]
    pub place: String,
    pub chapter: u32,
    pub start: u64,
    pub end: u64,
    pub words: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    pub turns: Vec<Turn>,
}

impl Paths {
    pub fn conversation(&self) -> std::path::PathBuf {
        self.dir.join("conversation.json")
    }
}

impl Conversation {
    /// The kept conversation. A missing or damaged file reads as no turns yet.
    pub fn load(paths: &Paths) -> Self {
        fs::read(paths.conversation()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    /// Adds a turn and keeps the last [`KEPT`].
    pub fn push(&mut self, turn: Turn) {
        self.turns.push(turn);
        let over = self.turns.len().saturating_sub(KEPT);
        self.turns.drain(..over);
    }

    /// Writes the file whole, through a temporary one, so a crash never leaves half of it.
    pub fn save(&self, paths: &Paths) -> io::Result<()> {
        fs::create_dir_all(&paths.dir)?;
        let temporary = paths.dir.join("conversation.json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(self).map_err(io::Error::other)?)?;
        fs::rename(temporary, paths.conversation())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn turn() -> impl Strategy<Value = Turn> {
        ("\\PC{0,40}", "\\PC{0,20}", "\\PC{0,80}", any::<bool>(), any::<u64>(), prop::collection::vec(("\\PC{0,10}", any::<u32>(), any::<u64>()), 0..3)).prop_map(
            |(asked, context, answer, edit, made, cites)| Turn {
                asked,
                context,
                answer,
                edit,
                made,
                citations: cites.into_iter().enumerate().map(|(n, (words, chapter, start))| Citation { n: n as u32 + 1, item: "b".into(), title: "Walden".into(), place: "Economy".into(), chapter, start, end: start.saturating_add(4), words }).collect(),
            },
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(40))]

        /// Whatever is said, the conversation reads back as it was kept, the last KEPT turns of it.
        #[test]
        fn the_conversation_survives_a_relaunch(turns in prop::collection::vec(turn(), 0..6), extra in 0usize..3) {
            let dir = tempfile_dir();
            let paths = Paths { dir: dir.clone() };
            let mut conversation = Conversation::load(&paths);
            prop_assert!(conversation.turns.is_empty());
            for turn in &turns {
                conversation.push(turn.clone());
            }
            for _ in 0..extra * KEPT {
                conversation.push(Turn::default());
            }
            conversation.save(&paths).unwrap();
            let back = Conversation::load(&paths);
            prop_assert!(back.turns.len() <= KEPT);
            prop_assert_eq!(&back, &conversation);
            if extra == 0 {
                prop_assert_eq!(back.turns, turns);
            }
            let _ = fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_damaged_file_reads_as_no_turns() {
        let dir = tempfile_dir();
        let paths = Paths { dir: dir.clone() };
        fs::create_dir_all(&dir).unwrap();
        fs::write(paths.conversation(), b"{ not json").unwrap();
        assert_eq!(Conversation::load(&paths), Conversation::default());
        let _ = fs::remove_dir_all(dir);
    }

    fn tempfile_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!("inkling-conversation-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)))
    }
}
