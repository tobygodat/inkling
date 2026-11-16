//! The documents in step between devices, with no saved text ever lost.
//!
//! The server keeps each document with a `version` it counts up on every write it accepts, and
//! takes a write only when it names the version it was made on (its `base`). This device keeps,
//! in `sync.json`, the version and the SHA-256 of the text it last agreed with the server on.
//! So a document has changed here when its text no longer hashes to its base, and on the server
//! when the version moved on.
//!
//! - Changed here only: the text goes up.
//! - Changed there only: the server's text comes down.
//! - Changed on both: the server's text is kept as a second document under a new id, with its
//!   own date, and this device's text goes up on the original id. The same text on both is no
//!   conflict.
//! - A delete is a write too, so a delete on one device and an edit on another keeps the edit.
//!
//! [`sync`] runs off the UI thread: it reads the files, talks to the server and returns what to
//! change on disk. [`apply`] makes those changes on the UI thread, each only while the file still
//! holds what [`sync`] read, so a keystroke typed meanwhile is never written over. A change
//! skipped there is a conflict at the next sync, and both texts are kept then.

use crate::api::{Api, Body, read};
use crate::{Error, Method};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// What this device last agreed with the server on, for each document.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// This install, as the server records who saved a document.
    pub device: String,
    pub documents: BTreeMap<String, Base>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Base {
    pub version: u64,
    /// SHA-256 of the text, in hex.
    pub hash: String,
}

/// A document as it is on this device.
#[derive(Clone, Debug, PartialEq)]
pub struct Local {
    pub id: String,
    pub text: String,
    /// When its file last changed, in milliseconds since the Unix epoch.
    pub saved_at: u64,
}

/// A document on the server, without its text.
#[derive(Clone, Debug, Deserialize)]
struct Listed {
    id: String,
    version: u64,
    #[serde(default)]
    deleted: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct Stored {
    text: String,
    version: u64,
    saved_at: u64,
    #[serde(default)]
    device: String,
    #[serde(default)]
    deleted: bool,
}

/// A change to make on this device's disk, if the file still holds what sync read.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// Write `text` with the time it was saved. `expect` is the hash the file must still have, or
    /// `None` when there must be no file.
    Write { id: String, text: String, saved_at: u64, expect: Option<String>, base: Base },
    /// The document was deleted on another device and not changed here.
    Remove { id: String, expect: String },
}

/// What a sync did: bases that hold whatever happens on disk, and the changes to make there.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// A document's new base, or `None` to forget it: it is gone everywhere.
    pub agreed: Vec<(String, Option<Base>)>,
    pub changes: Vec<Change>,
}

pub fn hash(text: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, text.as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// Brings this device and the server into step for every document either has. `fresh` makes a
/// new document id, for a conflict's second copy, from the ids already taken.
pub fn sync(api: &Api, state: &State, locals: &[Local], mut fresh: impl FnMut(&BTreeSet<String>) -> String) -> Result<Outcome, Error> {
    let remote: Vec<Listed> = api.json(Method::Get, "/v1/documents", Body::None)?;
    let remote: BTreeMap<String, Listed> = remote.into_iter().map(|d| (d.id.clone(), d)).collect();
    let local: BTreeMap<&str, &Local> = locals.iter().map(|l| (l.id.as_str(), l)).collect();
    let mut taken: BTreeSet<String> = remote.keys().cloned().chain(local.keys().map(|k| k.to_string())).chain(state.documents.keys().cloned()).collect();
    let ids = taken.clone();
    let mut out = Outcome::default();

    for id in &ids {
        let here = local.get(id.as_str()).copied();
        let base = state.documents.get(id);
        let there = remote.get(id);
        let changed_here = match (here, base) {
            (Some(l), Some(b)) => hash(&l.text) != b.hash,
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };
        let version = there.map_or(0, |r| r.version);
        let changed_there = version != base.map_or(0, |b| b.version);
        let live_there = there.is_some_and(|r| !r.deleted);

        match (changed_here, changed_there) {
            (false, false) => {}
            (true, false) => match here {
                Some(l) => {
                    if let Some(v) = put(api, &state.device, id, &l.text, l.saved_at, version, false)? {
                        out.agreed.push((id.clone(), Some(Base { version: v, hash: hash(&l.text) })));
                    }
                }
                None if live_there => {
                    if put(api, &state.device, id, "", crate::now_millis(), version, true)?.is_some() {
                        out.agreed.push((id.clone(), None));
                    }
                }
                None => out.agreed.push((id.clone(), None)),
            },
            (false, true) => match (here, live_there) {
                (Some(l), false) => out.changes.push(Change::Remove { id: id.clone(), expect: hash(&l.text) }),
                (None, false) => {}
                (_, true) => {
                    let r = get(api, id)?;
                    let base = Base { version: r.version, hash: hash(&r.text) };
                    out.changes.push(Change::Write { id: id.clone(), expect: here.map(|l| hash(&l.text)), text: r.text, saved_at: r.saved_at, base });
                }
            },
            (true, true) => {
                let r = if there.is_some() { Some(get(api, id)?) } else { None };
                match (here, r) {
                    // Deleted here, edited there: the edit stays.
                    (None, Some(r)) if !r.deleted => {
                        let base = Base { version: r.version, hash: hash(&r.text) };
                        out.changes.push(Change::Write { id: id.clone(), expect: None, text: r.text, saved_at: r.saved_at, base });
                    }
                    (None, _) => out.agreed.push((id.clone(), None)),
                    (Some(l), r) => {
                        let (version, theirs) = match r {
                            Some(r) if !r.deleted && hash(&r.text) != hash(&l.text) => (r.version, Some(r)),
                            Some(r) if !r.deleted => {
                                out.agreed.push((id.clone(), Some(Base { version: r.version, hash: hash(&l.text) })));
                                continue;
                            }
                            Some(r) => (r.version, None),
                            None => (0, None),
                        };
                        if let Some(r) = theirs {
                            // The other device's copy first, so it is safe on the server before
                            // this device's text takes the id.
                            let Some((copy, v)) = keep_copy(api, &r, &mut taken, &mut fresh)? else { continue };
                            out.changes.push(Change::Write {
                                id: copy,
                                expect: None,
                                base: Base { version: v, hash: hash(&r.text) },
                                text: r.text,
                                saved_at: r.saved_at,
                            });
                        }
                        if let Some(v) = put(api, &state.device, id, &l.text, l.saved_at, version, false)? {
                            out.agreed.push((id.clone(), Some(Base { version: v, hash: hash(&l.text) })));
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Puts `r` up under a new id, as the device that saved it. A few tries, in case another device
/// takes the same id at the same moment.
fn keep_copy(api: &Api, r: &Stored, taken: &mut BTreeSet<String>, fresh: &mut impl FnMut(&BTreeSet<String>) -> String) -> Result<Option<(String, u64)>, Error> {
    for _ in 0..3 {
        let copy = fresh(taken);
        taken.insert(copy.clone());
        if let Some(v) = put(api, &r.device, &copy, &r.text, r.saved_at, 0, false)? {
            return Ok(Some((copy, v)));
        }
    }
    Ok(None)
}

/// The new version, or `None` when the server has moved on since `base`: the next sync sorts it out.
fn put(api: &Api, device: &str, id: &str, text: &str, saved_at: u64, base: u64, deleted: bool) -> Result<Option<u64>, Error> {
    let body = json!({ "text": text, "saved_at": saved_at, "device": device, "base": base, "deleted": deleted });
    let response = api.answer(Method::Put, &format!("/v1/documents/{id}"), Body::Json(body))?;
    match response.status {
        200..=299 => Ok(Some(read::<serde_json::Value>(&response.body)?["version"].as_u64().unwrap_or(base + 1))),
        409 => Ok(None),
        _ => Err(crate::api::refused(&response)),
    }
}

fn get(api: &Api, id: &str) -> Result<Stored, Error> {
    api.json(Method::Get, &format!("/v1/documents/{id}"), Body::None)
}

/// The documents on this device's disk, as [`apply`] changes them.
pub trait Disk {
    fn read(&self, id: &str) -> Option<String>;
    fn write(&mut self, id: &str, text: &str, saved_at: u64) -> Result<(), String>;
    fn remove(&mut self, id: &str) -> Result<(), String>;
    /// Whether the app lets sync change this document now. One under review, say, is left.
    fn open_to_sync(&self, id: &str) -> bool {
        let _ = id;
        true
    }
}

/// Makes `outcome`'s changes on `disk` and records the bases in `state`. Returns the ids written
/// or removed, for the app to show.
pub fn apply(outcome: Outcome, state: &mut State, disk: &mut impl Disk) -> Vec<String> {
    for (id, base) in outcome.agreed {
        match base {
            Some(base) => state.documents.insert(id, base),
            None => state.documents.remove(&id),
        };
    }
    let mut touched = Vec::new();
    for change in outcome.changes {
        match change {
            Change::Write { id, text, saved_at, expect, base } => {
                if disk.open_to_sync(&id) && disk.read(&id).map(|t| hash(&t)) == expect && disk.write(&id, &text, saved_at).is_ok() {
                    state.documents.insert(id.clone(), base);
                    touched.push(id);
                }
            }
            Change::Remove { id, expect } => {
                if disk.open_to_sync(&id) && disk.read(&id).map(|t| hash(&t)) == Some(expect) && disk.remove(&id).is_ok() {
                    state.documents.remove(&id);
                    touched.push(id);
                }
            }
        }
    }
    touched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Fake;
    use proptest::prelude::*;

    /// One device: its documents on disk and what it agreed with the server.
    #[derive(Default)]
    struct Device {
        disk: BTreeMap<String, String>,
        state: State,
    }

    impl Disk for BTreeMap<String, String> {
        fn read(&self, id: &str) -> Option<String> {
            self.get(id).cloned()
        }
        fn write(&mut self, id: &str, text: &str, _: u64) -> Result<(), String> {
            self.insert(id.into(), text.into());
            Ok(())
        }
        fn remove(&mut self, id: &str) -> Result<(), String> {
            BTreeMap::remove(self, id);
            Ok(())
        }
    }

    fn token(fake: &Fake) -> String {
        let begun = crate::account::begin(fake, crate::FAKE_SERVER, "inkling://callback", "test").unwrap();
        begun.finish(fake, &crate::fake_way_back(&begun.url).unwrap()).unwrap().token(fake).unwrap()
    }

    fn run(fake: &Fake, token: &str, device: &Device) -> Outcome {
        let api = Api::new(fake, crate::FAKE_SERVER, token.to_string());
        let locals: Vec<Local> = device.disk.iter().map(|(id, text)| Local { id: id.clone(), text: text.clone(), saved_at: 1 }).collect();
        let mut n = 0;
        sync(&api, &device.state, &locals, |taken| loop {
            n += 1;
            let id = format!("copy{n}");
            if !taken.contains(&id) {
                return id;
            }
        })
        .unwrap()
    }

    #[derive(Clone, Debug)]
    enum Op {
        Edit(usize, usize),
        New(usize, usize),
        Delete(usize, usize),
        Sync(usize),
        /// A sync during which the writer edits a document, before its changes land.
        SyncWhileTyping(usize, usize),
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0..2usize, 0..4usize).prop_map(|(d, k)| Op::Edit(d, k)),
            (0..2usize, 0..3usize).prop_map(|(d, k)| Op::New(d, k)),
            (0..2usize, 0..4usize).prop_map(|(d, k)| Op::Delete(d, k)),
            (0..2usize).prop_map(Op::Sync),
            (0..2usize, 0..4usize).prop_map(|(d, k)| Op::SyncWhileTyping(d, k)),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Two devices, any sequence of edits, new documents (sometimes with the same id on
        /// both), deletes and syncs. Once both have synced, they hold the same documents, and
        /// every text a writer saved is there unless that writer replaced or deleted it. A
        /// conflict may keep a text its own device went on to replace, which loses nothing.
        #[test]
        fn no_saved_text_is_lost(ops in proptest::collection::vec(op(), 0..40)) {
            let fake = Fake::memory();
            let token = token(&fake);
            let mut devices = [Device::default(), Device::default()];
            devices[0].state.device = "mac".into();
            devices[1].state.device = "phone".into();
            let mut written = 0;
            let mut kept: BTreeSet<String> = BTreeSet::new();
            // The writer replaces the text of the `k`th document on `device` with a new one.
            fn edit(device: &mut Device, k: usize, written: &mut usize, kept: &mut BTreeSet<String>) {
                let Some(id) = device.disk.keys().nth(k % device.disk.len().max(1)).cloned() else { return };
                *written += 1;
                let text = format!("text {written}");
                kept.remove(&device.disk.insert(id, text.clone()).unwrap());
                kept.insert(text);
            }
            for op in ops {
                match op {
                    Op::Edit(d, k) => edit(&mut devices[d], k, &mut written, &mut kept),
                    Op::New(d, k) => {
                        let id = format!("doc{k}");
                        if let std::collections::btree_map::Entry::Vacant(slot) = devices[d].disk.entry(id) {
                            written += 1;
                            let text = format!("text {written}");
                            slot.insert(text.clone());
                            kept.insert(text);
                        }
                    }
                    Op::Delete(d, k) => {
                        if let Some(id) = devices[d].disk.keys().nth(k % devices[d].disk.len().max(1)).cloned() {
                            kept.remove(&devices[d].disk.remove(&id).unwrap());
                        }
                    }
                    Op::Sync(d) => {
                        let outcome = run(&fake, &token, &devices[d]);
                        let device = &mut devices[d];
                        apply(outcome, &mut device.state, &mut device.disk);
                    }
                    Op::SyncWhileTyping(d, k) => {
                        let outcome = run(&fake, &token, &devices[d]);
                        edit(&mut devices[d], k, &mut written, &mut kept);
                        let device = &mut devices[d];
                        apply(outcome, &mut device.state, &mut device.disk);
                    }
                }
            }
            for _ in 0..3 {
                for device in &mut devices {
                    let outcome = run(&fake, &token, device);
                    apply(outcome, &mut device.state, &mut device.disk);
                }
            }
            let texts = |d: &Device| { let mut t: Vec<String> = d.disk.values().cloned().collect(); t.sort(); t };
            prop_assert_eq!(texts(&devices[0]), texts(&devices[1]));
            let on_both: BTreeSet<String> = texts(&devices[0]).into_iter().collect();
            prop_assert!(on_both.is_superset(&kept), "lost {:?}", kept.difference(&on_both).collect::<Vec<_>>());
            prop_assert_eq!(texts(&devices[0]).len(), on_both.len(), "a text is on a device twice");
            let live = fake.with(|s| s.documents.values().filter(|d| !d.deleted).count());
            prop_assert_eq!(live, devices[0].disk.len());
        }
    }
}
