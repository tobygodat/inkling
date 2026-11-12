//! What VoiceOver reads of the page, from the AccessKit tree GPUI builds for the window. The run
//! sets `GPUI_A11Y_ACTIVE=1`, so the headless window builds that tree as it would for a screen
//! reader. Nothing turns VoiceOver on.

use super::*;
use serde_json::Value;

/// The draft every lane opens: a heading, both kinds of formula, a local and a remote image.
const DRAFT: &str = "# Ferry notes\n\nEnergy is $E = mc^2$ and a gull ![a gull](gull.svg) by ![the harbour](https://img.example/harbour.png).\n\n$$\n\\sum_{k=1}^{n} k\n$$\n\nThe caret starts in this last paragraph.";

/// What the text-area page reported at the commit before L2 (`014c23b`), dumped the same way:
/// role, actions, label and placeholder. Its value was the draft's source.
const TRUNK_ROLE: &str = "MultilineTextInput";
const TRUNK_ACTIONS: [&str; 2] = ["Focus", "SetValue"];
const TRUNK_PHONE_LABEL: &str = "Start writing";
/// The review at that commit reported no node of its own: the window and the ask field.
const TRUNK_REVIEW: [(&str, Option<&str>); 2] = [("TextInput", Some("Ask for fixes, words or a nudge")), ("Window", Some("Inkling"))];

const GULL: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="48" height="24"><ellipse cx="24" cy="12" rx="22" ry="10" fill="#8DA2B5"/></svg>"##;

struct Tree(Value);

impl Tree {
    fn nodes(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0["nodes"].as_object().into_iter().flatten()
    }

    fn page(&self) -> Option<(&String, &Value)> {
        self.nodes().find(|(_, n)| n["aria"]["role"] == TRUNK_ROLE)
    }

    fn aria<'a>(&'a self, key: &str) -> &'a Value {
        &self.0["nodes"][key]["aria"]
    }

    /// The page's children, in order.
    fn children(&self) -> Vec<&Value> {
        let Some((_, page)) = self.page() else { return vec![] };
        page["children"].as_array().into_iter().flatten().filter_map(|k| k.as_str()).map(|k| self.aria(k)).collect()
    }

    fn runs(&self) -> Vec<String> {
        self.children().into_iter().filter(|n| n["role"] == "TextRun").filter_map(|n| n["value"].as_str()).map(str::to_string).collect()
    }

    fn labels(&self) -> Vec<String> {
        self.children().into_iter().filter(|n| n["role"] != "TextRun").filter_map(|n| n["label"].as_str()).map(str::to_string).collect()
    }

    fn value(&self) -> Option<String> {
        self.page().and_then(|(_, n)| n["aria"]["value"].as_str()).map(str::to_string)
    }

    fn str<'a>(&'a self, field: &str) -> Option<&'a str> {
        self.page().and_then(|(_, n)| n["aria"][field].as_str())
    }

    fn actions(&self) -> Vec<String> {
        self.page().map(|(_, n)| n["aria"]["on_action"].as_array().into_iter().flatten().filter_map(|a| a.as_str()).map(str::to_string).collect()).unwrap_or_default()
    }

    /// The page node and its children as the tree holds them, beside the lane's screenshot.
    fn save(&self, dir: &Path, name: &str) {
        let page = self.page().map(|(_, n)| {
            let children: Vec<&Value> = n["children"].as_array().into_iter().flatten().filter_map(|c| c.as_str()).map(|c| self.aria(c)).collect();
            serde_json::json!({ "focused": self.page_focused(), "page": n["aria"], "children": children })
        });
        let nodes: Vec<&Value> = self.nodes().map(|(_, n)| &n["aria"]).collect();
        let json = page.unwrap_or_else(|| serde_json::json!({ "page": null, "nodes": nodes }));
        std::fs::write(dir.join(format!("{name}.json")), serde_json::to_string_pretty(&json).expect("json") + "\n").expect("write the tree");
    }

    fn page_focused(&self) -> bool {
        self.page().is_some_and(|(k, _)| self.0["gpui_focus"] == k.as_str())
    }
}

impl Snap {
    fn tree(&mut self) -> Tree {
        self.frame();
        let json = self.with(|w, _| w.debug_a11y_tree_json()).unwrap_or_else(|| fail("the window built an accessibility tree; is GPUI_A11Y_ACTIVE=1 set?"));
        Tree(serde_json::from_str(&json).unwrap_or_else(|e| fail(&format!("the tree is JSON: {e}"))))
    }

    fn reported_selection(&mut self) -> [(usize, usize); 2] {
        self.read(|v, cx| v.page().live().read(cx).reported_selection())
    }
}

/// The source byte at character `index` of run `run`, counted through the tree's own runs.
fn byte_at(runs: &[String], (run, index): (usize, usize)) -> Option<usize> {
    let before: usize = runs.get(..run)?.iter().map(String::len).sum();
    let text = runs.get(run)?;
    let within = text.char_indices().map(|(i, _)| i).chain([text.len()]).nth(index)?;
    Some(before + within)
}

pub fn run(dir: &Path) {
    std::fs::create_dir_all(dir).expect("screenshot dir");
    let paths = Paths::mac();
    std::fs::create_dir_all(&paths.dir).expect("data dir");
    std::fs::write(paths.dir.join("gull.svg"), GULL).expect("write the image");
    paths.save_draft(DRAFT).expect("write the draft");
    let mut s = Snap::open(dir);
    let caret = DRAFT.find("starts in").expect("caret text") + 3;
    s.select(caret..caret);

    let l = "a11y-01";
    let t = s.tree();
    lane(t.page().is_some(), l, "the page is one node in the accessibility tree");
    lane(t.str("role") == Some(TRUNK_ROLE), l, &format!("its role is {TRUNK_ROLE}, as the text-area page's was"));
    lane(t.value().as_deref() == Some(DRAFT), l, "its value is the draft's source, as the text-area page's was");
    lane(t.actions() == TRUNK_ACTIONS, l, &format!("it offers {TRUNK_ACTIONS:?}, as the text-area page did"));
    lane(t.str("label").is_none() && t.str("placeholder").is_none(), l, "on the Mac it has no label or placeholder, as the text-area page had none");
    lane(t.page_focused(), l, "it is the focused node");
    lane(t.runs().concat() == DRAFT, l, "its text runs join up to the source");
    let first_value = t.value();
    s.shot("a11y-01-node");
    s.tree().save(dir, "a11y-01-node");

    let l = "a11y-04";
    let labels = t.labels();
    lane(labels.iter().any(|x| x == "Formula E = mc^2"), l, &format!("the inline formula reads \"Formula E = mc^2\" {labels:?}"));
    lane(labels.iter().any(|x| x == "Formula \\sum_{k=1}^{n} k"), l, "the block formula reads \"Formula\" and its source");
    s.shot("a11y-04-formula");
    s.tree().save(dir, "a11y-04-formula");

    let l = "a11y-05";
    lane(labels.iter().any(|x| x == "a gull"), l, "the local image reads its alt text");
    lane(labels.iter().any(|x| x == "the harbour"), l, "the remote image, drawn as its alt text, reads its alt text");
    s.shot("a11y-05-image");
    s.tree().save(dir, "a11y-05-image");

    let l = "a11y-06";
    s.update(|v, w, cx| v.set_appearance(|a| (a.markdown, a.maths) = (false, false), w, cx));
    let t = s.tree();
    lane(s.live().formulas.is_empty(), l, "with both switches off no formula is drawn");
    lane(t.value() == first_value, l, "the value is the same as with both switches on");
    lane(t.labels().is_empty(), l, "no formula or image node is left");
    s.shot("a11y-06-plain");
    s.tree().save(dir, "a11y-06-plain");
    s.update(|v, w, cx| v.set_appearance(|a| (a.markdown, a.maths) = (true, true), w, cx));

    let l = "a11y-02";
    s.select(caret..caret);
    let runs = s.tree().runs();
    let [anchor, head] = s.reported_selection();
    lane(byte_at(&runs, anchor) == Some(caret) && anchor == head, l, &format!("the caret is reported at its source byte {caret} ({head:?})"));
    s.press("right");
    s.press("right");
    let runs = s.tree().runs();
    let [_, head] = s.reported_selection();
    lane(byte_at(&runs, head) == Some(s.cursor()) && s.cursor() == caret + 2, l, "after two Right presses the caret is reported two bytes on");
    let from = DRAFT.find("gull").expect("gull");
    s.select(from..caret);
    let runs = s.tree().runs();
    let [anchor, head] = s.reported_selection();
    lane(byte_at(&runs, anchor) == Some(from) && byte_at(&runs, head) == Some(caret), l, "a selection across three blocks is reported from its anchor to its head");
    lane(anchor.0 != head.0, l, "its ends are in different runs");
    s.shot("a11y-02-selection");
    s.tree().save(dir, "a11y-02-selection");

    let l = "a11y-03";
    let at = DRAFT.find("starts in").expect("word");
    s.select(at..at);
    s.input("ferry ");
    let t = s.tree();
    lane(t.value().as_deref() == Some(s.text().as_str()) && s.text().contains("caret ferry starts"), l, "a typed word is in the value");
    lane(t.runs().concat() == s.text(), l, "and in the text runs");
    s.shot("a11y-03-value");
    s.tree().save(dir, "a11y-03-value");

    let l = "a11y-07";
    s.update(|v, _, cx| {
        let len = v.page().value(cx).len();
        v.page().replace(0..len, GRAMMAR, cx);
    });
    s.frame();
    s.click("fix-grammar");
    s.wait(10_000, reviewing);
    let t = s.tree();
    let mut nodes: Vec<(String, Option<String>)> =
        t.nodes().map(|(_, n)| (n["aria"]["role"].as_str().unwrap_or_default().to_string(), n["aria"]["label"].as_str().map(str::to_string))).collect();
    nodes.sort();
    let trunk: Vec<(String, Option<String>)> = TRUNK_REVIEW.iter().map(|(r, l)| (r.to_string(), l.map(str::to_string))).collect();
    lane(nodes == trunk, l, &format!("the review reports the same nodes as the text-area page's review {nodes:?}"));
    s.shot("a11y-07-review");
    s.tree().save(dir, "a11y-07-review");

    let l = "a11y-10";
    s.click("accept-rest");
    let t = s.tree();
    lane(!reviewing(&s.revision()) && t.value().as_deref() == Some(s.text().as_str()), l, "after the review the page node is back with the accepted text");
    lane(t.page_focused(), l, "and it has focus again");
    s.shot("a11y-10-flow");
    s.tree().save(dir, "a11y-10-flow");

    phone(dir);
}

/// The phone layout's page reads the same, with the label the text-area page had there.
fn phone(dir: &Path) {
    let l = "a11y-01";
    let mut s = Snap::open_as(dir, FormFactor::Phone);
    let t = s.tree();
    lane(t.str("role") == Some(TRUNK_ROLE) && t.actions() == TRUNK_ACTIONS, l, "the phone layout's page has the same role and actions");
    lane(t.str("label") == Some(TRUNK_PHONE_LABEL) && t.str("placeholder") == Some(TRUNK_PHONE_LABEL), l, "and its label and placeholder are \"Start writing\", as the text-area page's were");
    lane(t.value().as_deref() == Some(s.text().as_str()), l, "and its value is the source");
}
