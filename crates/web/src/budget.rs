//! A first pass that asks the real tree builder how big a page will get,
//! with the budget enforced inside it.
//!
//! html5ever re-opens the active formatting elements (`<b>`, `<a>`, `<font>`
//! ...) whenever text follows blocks that closed them, and attributes that
//! differ defeat its limit of three alike: `<div>` x M, then `<b a=i>` for i in
//! 0..N, then `</div>x` x M makes N x M elements from N + M tags (N = M = 2,000:
//! 4 million nodes, 505 MB, 1.2 s; at the byte caps, tens of GB). The size of
//! the tree cannot be told from the bytes, and checking it after the parse is
//! too late.
//!
//! So the page is parsed once by html5ever itself (the version `dom_query`
//! drives, with the options `dom_query` uses: scripting off) into a counting
//! [`TreeSink`] of ours that keeps a minimal arena (name, parent, depth,
//! template contents) up to [`MAX_NODES`] nodes *created* (a re-opened
//! formatting element is a created node), and past that stops allocating: it
//! hands out one shared dummy handle, ignores appends and raises a flag. The
//! page is fed in chunks of [`CHUNK`] bytes and the driver stops at once after
//! the chunk that raised the flag, so memory is bounded by the budget and the
//! time by one chunk's worth of cheap calls. It stops, too, when an element is
//! appended deeper than [`crate::tree::MAX_NESTING`]: the tree builder's time
//! is quadratic in depth, and the exact depth is checked afterwards by
//! [`crate::tree::measure`].
//!
//! A page that passes is parsed again by `dom_query` (the tree the readability
//! pass needs), so a page that passes costs two parses, both bounded.

use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};
use std::rc::Rc;

use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeBuilderOpts};
use html5ever::{Attribute, ParseOpts, QualName, local_name, ns};

/// The most nodes (elements, comments, text, doctype) a page may create: past
/// it the page is refused. Measured: `dom_query` holds about 130 KB per 1,000
/// nodes (150,000 nodes: about 20 MB, a parse under a second).
pub const MAX_NODES: usize = 150_000;

/// The most nodes a page may create and still be given to the readers that
/// build on a tree. `html2text` holds about 1 MB per 1,000 nodes while it
/// renders (measured: 16,000 nodes 23 MB, 50,000 nodes 63 MB, 90,000 nodes
/// 104 MB peak, the page itself 7 to 15 MB of that), so a tree of at most
/// this many nodes keeps a read under about 40 MB. A page with more, up to
/// [`MAX_NODES`], is read as plain text.
pub const MAX_TREE_NODES: usize = 30_000;

/// The bytes fed to the parser at a time. After the chunk that passes the
/// budget the parser still runs to the end of that chunk, on a sink that
/// allocates nothing: measured on the worst input for that (paragraphs that
/// each re-open 500 formatting elements), 2,048 bytes cost 0.5 s, 512 cost
/// 15 ms and 128 cost 19 ms in release.
pub const CHUNK: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Within the budget (the exact depth is still checked on the real tree).
    Ok,
    /// More nodes were created than [`MAX_NODES`].
    TooMany,
    /// An element was appended deeper than the nesting limit.
    TooDeep,
}

const NONE: u32 = u32::MAX;
/// The shared dummy handle and the document's, in every arena.
const DOCUMENT: u32 = 0;
const DUMMY: u32 = 1;

struct Node {
    /// `Some` for an element.
    name: Option<QualName>,
    parent: u32,
    depth: u32,
    template: u32,
    mathml_annotation_xml_integration_point: bool,
    /// The last child appended is a text node (the next text merges into it).
    last_is_text: bool,
}

impl Node {
    fn new(name: Option<QualName>) -> Self {
        Node {
            name,
            parent: NONE,
            depth: 0,
            template: NONE,
            mathml_annotation_xml_integration_point: false,
            last_is_text: false,
        }
    }
}

/// What the driver reads back.
#[derive(Default)]
struct State {
    /// Bytes of the page fed to the parser so far.
    fed: Cell<usize>,
    created: Cell<usize>,
    too_many: Cell<bool>,
    too_deep: Cell<bool>,
}

pub struct CountingSink {
    nodes: RefCell<Vec<Node>>,
    budget: usize,
    max_depth: u32,
    state: Rc<State>,
}

impl CountingSink {
    fn new(budget: usize, max_depth: usize) -> (Self, Rc<State>) {
        let state = Rc::new(State::default());
        let dummy_name = QualName::new(None, ns!(html), local_name!("div"));
        let nodes = vec![Node::new(None), Node::new(Some(dummy_name))];
        (
            CountingSink {
                nodes: RefCell::new(nodes),
                budget,
                max_depth: max_depth as u32,
                state: state.clone(),
            },
            state,
        )
    }

    fn over(&self) -> bool {
        self.state.too_many.get()
    }

    /// Make a node, or (past the budget) raise the flag and return the dummy.
    fn alloc(&self, node: Node) -> u32 {
        if self.over() {
            return DUMMY;
        }
        let n = self.state.created.get() + 1;
        if n > self.budget {
            self.state.too_many.set(true);
            return DUMMY;
        }
        self.state.created.set(n);
        let mut nodes = self.nodes.borrow_mut();
        let id = nodes.len() as u32;
        nodes.push(node);
        id
    }

    /// Count a node that is not kept (text appended, a doctype).
    fn count_only(&self) {
        if self.over() {
            return;
        }
        let n = self.state.created.get() + 1;
        if n > self.budget {
            self.state.too_many.set(true);
        } else {
            self.state.created.set(n);
        }
    }

    fn set_parent(&self, parent: u32, child: u32) {
        if self.over() || child == DUMMY || parent == DUMMY {
            return;
        }
        let mut nodes = self.nodes.borrow_mut();
        let depth = nodes[parent as usize].depth + 1;
        let is_element = nodes[child as usize].name.is_some();
        {
            let c = &mut nodes[child as usize];
            c.parent = parent;
            c.depth = depth;
        }
        nodes[parent as usize].last_is_text = false;
        if is_element && depth > self.max_depth {
            self.state.too_deep.set(true);
        }
    }
}

impl html5ever::interface::TreeSink for CountingSink {
    type Handle = u32;
    type Output = ();
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) {}

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> u32 {
        DOCUMENT
    }

    /// The name the tree builder reads on every decision. A node that is not
    /// an element answers the dummy's name rather than panic.
    fn elem_name<'a>(&'a self, target: &'a u32) -> Ref<'a, QualName> {
        Ref::map(self.nodes.borrow(), |nodes| {
            nodes[*target as usize]
                .name
                .as_ref()
                .or(nodes[DUMMY as usize].name.as_ref())
                .expect("the dummy is an element")
        })
    }

    fn create_element(&self, name: QualName, _attrs: Vec<Attribute>, flags: ElementFlags) -> u32 {
        let mut node = Node::new(Some(name));
        node.mathml_annotation_xml_integration_point =
            flags.mathml_annotation_xml_integration_point;
        if flags.template {
            // The template's contents: a fragment of its own, parentless.
            let contents = self.alloc(Node::new(None));
            node.template = contents;
        }
        self.alloc(node)
    }

    fn create_comment(&self, _text: StrTendril) -> u32 {
        self.alloc(Node::new(None))
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> u32 {
        self.alloc(Node::new(None))
    }

    fn append(&self, parent: &u32, child: NodeOrText<u32>) {
        match child {
            NodeOrText::AppendNode(c) => self.set_parent(*parent, c),
            NodeOrText::AppendText(_) => {
                // Text merges into a text node that is already the last child.
                let merge = !self.over()
                    && *parent != DUMMY
                    && self.nodes.borrow()[*parent as usize].last_is_text;
                if !merge {
                    self.count_only();
                    if !self.over() && *parent != DUMMY {
                        self.nodes.borrow_mut()[*parent as usize].last_is_text = true;
                    }
                }
            }
        }
    }

    /// Whether the element has a parent decides where the child goes.
    fn append_based_on_parent_node(
        &self,
        element: &u32,
        prev_element: &u32,
        child: NodeOrText<u32>,
    ) {
        let has_parent = self
            .nodes
            .borrow()
            .get(*element as usize)
            .is_some_and(|n| n.parent != NONE);
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, _n: StrTendril, _p: StrTendril, _s: StrTendril) {
        self.count_only();
    }

    fn get_template_contents(&self, target: &u32) -> u32 {
        match self
            .nodes
            .borrow()
            .get(*target as usize)
            .map(|n| n.template)
        {
            Some(t) if t != NONE => t,
            // The dummy, or a node whose creation was over the budget.
            _ => DUMMY,
        }
    }

    /// Handles are indexes. The dummy handed out past the budget is never the
    /// same node as anything, itself included: the algorithms that look for an
    /// element on the stack of open elements (the adoption agency) then do not
    /// find it and take their "not there" path instead of looping on a handle
    /// that stands for many nodes.
    fn same_node(&self, x: &u32, y: &u32) -> bool {
        x == y && *x != DUMMY
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &u32, child: NodeOrText<u32>) {
        let parent = self
            .nodes
            .borrow()
            .get(*sibling as usize)
            .map_or(NONE, |n| n.parent);
        match child {
            NodeOrText::AppendNode(c) => {
                if parent != NONE {
                    self.set_parent(parent, c);
                }
            }
            NodeOrText::AppendText(_) => self.count_only(),
        }
    }

    fn add_attrs_if_missing(&self, _target: &u32, _attrs: Vec<Attribute>) {}

    fn remove_from_parent(&self, target: &u32) {
        if !self.over()
            && *target != DUMMY
            && let Some(n) = self.nodes.borrow_mut().get_mut(*target as usize)
        {
            n.parent = NONE;
        }
    }

    /// The children are not listed here; their parent links keep pointing at
    /// a parent (which is all `append_based_on_parent_node` asks), and the
    /// exact depth of the final tree is measured on the real one.
    fn reparent_children(&self, _node: &u32, _new_parent: &u32) {}

    fn is_mathml_annotation_xml_integration_point(&self, handle: &u32) -> bool {
        self.nodes
            .borrow()
            .get(*handle as usize)
            .is_some_and(|n| n.mathml_annotation_xml_integration_point)
    }
}

/// Run `html` through the tree builder with the node budget and the depth
/// limit enforced inside it; the outcome and the nodes created.
pub fn run(html: &str, budget: usize, max_depth: usize, chunk: usize) -> (Outcome, usize) {
    let (sink, state) = CountingSink::new(budget, max_depth);
    // The options `dom_query::Document::from` parses with.
    let opts = ParseOpts {
        tokenizer: Default::default(),
        tree_builder: TreeBuilderOpts {
            scripting_enabled: false,
            ..Default::default()
        },
    };
    let parser = html5ever::parse_document(sink, opts);
    // A parser that panics on a sink that has stopped allocating is a page
    // that made too many nodes; a panic before that is the parser's own and
    // goes on up (the caller reports the page unreadable).
    let driven = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drive(parser, html, &state, chunk)
    }));
    let outcome = match driven {
        Ok(o) => o,
        Err(p) if state.too_many.get() => {
            drop(p);
            Outcome::TooMany
        }
        Err(p) => std::panic::resume_unwind(p),
    };
    (outcome, state.created.get())
}

fn drive(
    mut parser: html5ever::Parser<CountingSink>,
    html: &str,
    state: &Rc<State>,
    chunk: usize,
) -> Outcome {
    let mut rest = html;
    while !rest.is_empty() {
        // A chunk ends on a character boundary.
        let mut end = chunk.min(rest.len());
        while !rest.is_char_boundary(end) {
            end += 1;
        }
        let (now, later) = rest.split_at(end);
        parser.process(StrTendril::from_slice(now));
        state.fed.set(state.fed.get() + now.len());
        rest = later;
        if state.too_many.get() || state.too_deep.get() {
            break;
        }
    }
    let outcome = if state.too_many.get() {
        Outcome::TooMany
    } else if state.too_deep.get() {
        Outcome::TooDeep
    } else {
        Outcome::Ok
    };
    if outcome == Outcome::Ok {
        let _ = parser.finish();
        if state.too_many.get() {
            return Outcome::TooMany;
        }
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pile(n: usize, m: usize) -> String {
        let attrs: String = (0..n).map(|i| format!("<b a={i}>")).collect();
        format!("<p>{attrs}{}", "<p>x".repeat(m))
    }

    /// The sink stops allocating at the budget: the arena never holds more
    /// than the budget (plus the document, the dummy and template fragments),
    /// however many nodes the page asks for, and the flag is raised.
    #[test]
    fn the_sink_stops_allocating_at_the_budget() {
        // 400 formatting elements re-opened by each of 5,000 paragraphs: two
        // million nodes asked for.
        let html = pile(400, 5_000);
        let budget = 3_000;
        let (sink, state) = CountingSink::new(budget, usize::MAX >> 1);
        let parser = html5ever::parse_document(
            sink,
            ParseOpts {
                tokenizer: Default::default(),
                tree_builder: TreeBuilderOpts {
                    scripting_enabled: false,
                    ..Default::default()
                },
            },
        );
        let out = drive(parser, &html, &state, CHUNK);
        assert_eq!(out, Outcome::TooMany);
        assert!(state.too_many.get());
        assert_eq!(state.created.get(), budget, "counted exactly to the budget");
        // The driver stopped feeding at the chunk that raised the flag: it did
        // not read the rest of a two-megabyte page.
        assert!(
            state.fed.get() < html.len() / 4,
            "fed {} of {}",
            state.fed.get(),
            html.len()
        );
    }

    /// Through `run`: the outcomes, the count, and a page within the budget.
    #[test]
    fn run_says_ok_too_many_or_too_deep() {
        let ok = run("<p>a <b>b</b> <ul><li>c<li>d</ul>", MAX_NODES, 512, CHUNK);
        assert_eq!(ok.0, Outcome::Ok);
        assert_eq!(
            run(&pile(400, 5_000), MAX_NODES, 512, CHUNK).0,
            Outcome::TooMany
        );
        let deep = format!("{}x", "<div>".repeat(2_000));
        assert_eq!(run(&deep, MAX_NODES, 512, CHUNK).0, Outcome::TooDeep);
        // Non-ASCII text is cut on character boundaries.
        let wide = "é".repeat(5_000);
        assert_eq!(run(&wide, MAX_NODES, 512, 7).0, Outcome::Ok);
        // The count is the nodes created.
        let (o, n) = run("<p>a<b>b</b></p>", MAX_NODES, 512, CHUNK);
        assert_eq!(o, Outcome::Ok);
        assert!((5..=12).contains(&n), "{n}");
    }
}
