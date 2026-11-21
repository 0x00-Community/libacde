use blitz_dom::style::computed_values::visibility::T as Visibility;
use blitz_dom::{Node, NodeData, NodeId};
use serde::Serialize;

use super::{Control, Dom};
use crate::render::Rect;
use crate::render::engine::Engine;
use crate::render::geometry::Geometry;

const NAME_MAX: usize = 120;
const VALUE_MAX: usize = 200;
const CHILDREN_MAX: usize = 60;

#[derive(Serialize)]
struct Tree {
    hidden: usize,
    truncated: bool,
    nodes: Vec<Sem>,
}

#[derive(Serialize)]
struct Sem {
    #[serde(skip)]
    node: NodeId,
    id: u32,
    role: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    states: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<u8>,
    #[serde(serialize_with = "rounded")]
    rect: Rect,
    #[serde(skip_serializing_if = "is_zero")]
    more: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<Sem>,
}

fn rounded<S: serde::Serializer>(r: &Rect, s: S) -> Result<S::Ok, S::Error> {
    [r.x, r.y, r.width, r.height].map(|v| v.round_ties_even() as i64).serialize(s)
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

fn number(list: &mut [Sem], ids: &mut dyn FnMut(NodeId) -> u32) {
    for sem in list {
        sem.id = ids(sem.node);
        number(&mut sem.children, ids);
    }
}

struct Walk<'a> {
    e: &'a Engine,
    geometry: Geometry<'a>,
    focus: Option<NodeId>,
    budget: usize,
    hidden: usize,
    offscreen: bool,
    invisible: bool,
    faded: bool,
}

impl Dom {
    pub fn semantics(&self, root: u32, limit: u32, offscreen: bool, invisible: bool) -> crate::render::Result<String> {
        self.read(|e| {
            let root = match root {
                super::NONE => e.doc.try_root_element().map(|r| r.id),
                id => Some(e.n(id)?),
            };
            let mut tree = Tree { hidden: 0, truncated: false, nodes: Vec::new() };
            if let Some(root) = root {
                e.resolved();
                let viewport = e.doc.viewport();
                let scale = viewport.scale_f64();
                let screen = kurbo::Rect::new(0.0, 0.0, f64::from(viewport.window_size.0) / scale, f64::from(viewport.window_size.1) / scale);
                let mut walk = Walk {
                    e,
                    geometry: Geometry { doc: &e.doc },
                    focus: e.doc.get_focussed_node_id(),
                    budget: limit.max(1) as usize,
                    hidden: 0,
                    offscreen,
                    invisible,
                    faded: false,
                };
                tree.nodes = walk.element(root, screen, &mut tree.truncated);
                tree.hidden = walk.hidden;
                number(&mut tree.nodes, &mut |n| e.ids.id_of(n));
            }
            serde_json::to_string(&tree).map_err(|err| crate::render::Error::new(err.to_string()))
        })
    }
}

fn option_label(e: &Engine, option: NodeId) -> String {
    let node = e.doc.get_node(option);
    match node.and_then(|n| n.attr(blitz_dom::local_name!("label"))).filter(|l| !l.is_empty()) {
        Some(label) => label.to_owned(),
        None => node.map(|n| n.text_content()).unwrap_or_default(),
    }
}

fn collapse(text: &str, max: usize) -> String {
    let mut out = String::new();
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
        if out.chars().count() > max {
            break;
        }
    }
    if out.chars().count() > max {
        out = out.chars().take(max).collect::<String>() + "…";
    }
    out
}

const SKIPPED: [&str; 9] = ["script", "style", "template", "head", "meta", "link", "noscript", "title", "iframe"];

const INTERACTIVE: [&str; 19] = [
    "button",
    "link",
    "textbox",
    "searchbox",
    "checkbox",
    "radio",
    "combobox",
    "slider",
    "spinbutton",
    "switch",
    "tab",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "treeitem",
    "clickable",
    "listbox",
    "scrollbar",
];

const FROM_CONTENT: [&str; 17] = [
    "button",
    "link",
    "heading",
    "tab",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "option",
    "treeitem",
    "cell",
    "columnheader",
    "rowheader",
    "clickable",
    "checkbox",
    "radio",
    "switch",
    "tooltip",
];

impl<'a> Walk<'a> {
    fn node(&self, id: NodeId) -> Option<&'a Node> {
        self.e.doc.get_node(id)
    }

    fn element(&mut self, id: NodeId, clip: kurbo::Rect, truncated: &mut bool) -> Vec<Sem> {
        let Some(n) = self.node(id) else { return Vec::new() };
        let Some(e) = n.element_data() else { return Vec::new() };
        let tag = e.name.local.to_string();
        if SKIPPED.contains(&tag.as_str()) {
            return Vec::new();
        }
        let attr = |name: &str| e.attrs().iter().find(|a| &*a.name.local == name).map(|a| a.value.to_string());
        if attr("aria-hidden").as_deref() == Some("true") || attr("inert").is_some() {
            return Vec::new();
        }
        let Some(style) = n.primary_styles() else { return Vec::new() };
        if style.get_box().display.is_none() {
            return Vec::new();
        }
        let transparent = style.get_effects().opacity <= 0.0;
        let unseen = style.clone_visibility() != Visibility::Visible;
        if (transparent || unseen) && !self.invisible {
            return Vec::new();
        }
        let faded = self.faded;
        let transparent = transparent || faded;
        let rect = self.geometry.client_rect(id).unwrap_or_default();
        let sized = rect.width > 0.0 && rect.height > 0.0;
        let visible = kurbo::Rect::new(rect.x, rect.y, rect.x + rect.width, rect.y + rect.height).intersect(clip);
        if sized && visible.is_zero_area() && !self.offscreen {
            self.hidden += 1;
            return Vec::new();
        }
        if self.budget == 0 {
            *truncated = true;
            return Vec::new();
        }
        let resolved = |p: &str| self.e.doc.resolved_style_value(id, p);
        let overflow = [resolved("overflow-x"), resolved("overflow-y")];
        let clips = sized && overflow.iter().any(|o| o != "visible" && !o.is_empty());
        let inner = if clips { visible } else { clip };
        let role = self.role(n, &tag, &attr);
        let mut states = self.states(n, &tag, &attr, role.as_deref());
        if clips && overflow.iter().any(|o| o == "auto" || o == "scroll") {
            let l = n.final_layout();
            if l.scroll_height() > 0.5 || l.scroll_width() > 0.5 {
                states.push("scrollable");
            }
        }
        let control = self.e.control(id);
        let content_named = role.as_deref().is_some_and(|r| FROM_CONTENT.contains(&r)) && !matches!(control, Control::Editor(_) | Control::Select);
        let name = self.name(n, &tag, &attr, content_named);
        let value = self.value(id, &tag, &attr, control);
        let level = match tag.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => tag[1..].parse().ok(),
            _ => None,
        }
        .or_else(|| attr("aria-level").and_then(|l| l.parse().ok()));
        let mut children = Vec::new();
        let mut more = 0;
        let child_ids: Vec<NodeId> = n.children.to_vec();
        let named_by_content = content_named && !name.is_empty();
        self.faded = transparent;
        let mut ended_blank = false;
        for child in child_ids {
            let Some(c) = self.node(child) else { continue };
            if let NodeData::Text(t) = &c.data {
                if named_by_content || !sized && rect.width <= 0.0 {
                    continue;
                }
                let text = collapse(&t.content, NAME_MAX);
                let gap = ended_blank || t.content.starts_with(char::is_whitespace);
                if !t.content.is_empty() {
                    ended_blank = t.content.ends_with(char::is_whitespace);
                }
                if text.is_empty() {
                    continue;
                }
                match children.last_mut() {
                    Some(Sem { role, node: last, name, .. }) if role == "text" && *last == id => {
                        if name.chars().count() < NAME_MAX {
                            if gap {
                                name.push(' ');
                            }
                            name.push_str(&text);
                            *name = collapse(name, NAME_MAX);
                        }
                    }
                    _ => children.push(Sem {
                        node: id,
                        id: 0,
                        role: "text".into(),
                        name: text,
                        value: None,
                        states: Vec::new(),
                        rect,
                        level: None,
                        children: Vec::new(),
                        more: 0,
                    }),
                }
                continue;
            }
            if children.len() >= CHILDREN_MAX || self.budget == 0 {
                more += 1;
                continue;
            }
            let found = self.element(child, inner, truncated);
            if named_by_content {
                children.extend(found.into_iter().filter(|s| s.role != "text"));
            } else {
                children.extend(found);
            }
        }
        self.faded = faded;
        if control == Control::Select {
            children.extend(self.list_box_options(id, truncated));
            if !self.e.display_size(id).1 {
                let open = self.e.picker_rects(id);
                states.push(if open.is_empty() { "collapsed" } else { "expanded" });
                children.extend(self.options_at(open, truncated));
            }
        }
        if more > 0 {
            *truncated = true;
        }
        let meaningful = role.is_some() || !states.is_empty() && states != ["scrollable"] || value.is_some();
        let generic_named = role.is_none() && (attr("aria-label").is_some() || attr("title").is_some()) && !name.is_empty();
        let scroller = states.contains(&"scrollable");
        if !(meaningful || generic_named || scroller) {
            if more > 0
                && let Some(last) = children.last_mut()
            {
                last.more += more;
            }
            return children;
        }
        self.budget = self.budget.saturating_sub(1);
        let role = role.unwrap_or_else(|| "generic".into());
        if role == "generic" && children.len() == 1 && name.is_empty() && !scroller {
            return children;
        }
        if children.is_empty() && name.is_empty() && value.is_none() && more == 0 && !INTERACTIVE.contains(&role.as_str()) {
            return Vec::new();
        }
        if transparent {
            states.push("transparent");
        }
        if unseen {
            states.push("invisible");
        }
        vec![Sem { node: id, id: 0, role, name, value, states, rect: if sized { rect } else { Rect::default() }, level, children, more }]
    }

    fn list_box_options(&mut self, select: NodeId, truncated: &mut bool) -> Vec<Sem> {
        let rows = self.e.list_box_rows(select).into_iter().map(|(o, _, r)| (o, r)).collect();
        self.options_at(rows, truncated)
    }

    fn options_at(&mut self, rows: Vec<(NodeId, Rect)>, truncated: &mut bool) -> Vec<Sem> {
        let mut out = Vec::new();
        for (option, rect) in rows {
            if self.budget == 0 {
                *truncated = true;
                break;
            }
            self.budget -= 1;
            let mut states = Vec::new();
            if self.e.selectedness(option) {
                states.push("selected");
            }
            if self.e.option_disabled(option) {
                states.push("disabled");
            }
            let name = collapse(&option_label(self.e, option), NAME_MAX);
            out.push(Sem { node: option, id: 0, role: "option".into(), name, value: None, states, level: None, rect, more: 0, children: Vec::new() });
        }
        out
    }

    fn role(&self, n: &Node, tag: &str, attr: &dyn Fn(&str) -> Option<String>) -> Option<String> {
        if let Some(role) = attr("role").and_then(|r| r.split_whitespace().next().map(str::to_ascii_lowercase)) {
            match role.as_str() {
                "presentation" | "none" | "generic" => return None,
                _ => return Some(role),
            }
        }
        let input = || attr("type").map(|t| t.to_ascii_lowercase()).unwrap_or_else(|| "text".into());
        let role = match tag {
            "a" if attr("href").is_some() => "link",
            "button" | "summary" => "button",
            "input" => match input().as_str() {
                "hidden" => return None,
                "checkbox" => "checkbox",
                "radio" => "radio",
                "range" => "slider",
                "number" => "spinbutton",
                "search" => "searchbox",
                "button" | "submit" | "reset" | "image" | "file" | "color" => "button",
                _ => "textbox",
            },
            "textarea" => "textbox",
            "select" if self.e.display_size(n.id).1 => "listbox",
            "select" => "combobox",
            "option" => "option",
            "img" => match attr("alt") {
                Some(alt) if !alt.trim().is_empty() => "img",
                _ => return None,
            },
            "svg" => match n.children.iter().any(|&c| self.node(c).is_some_and(|c| c.element_data().is_some_and(|e| &*e.name.local == "title"))) {
                true => "img",
                false => return None,
            },
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
            "ul" | "ol" | "menu" => "list",
            "li" => "listitem",
            "nav" => "navigation",
            "main" => "main",
            "aside" => "complementary",
            "section" if attr("aria-label").is_some_and(|l| !l.trim().is_empty()) || attr("aria-labelledby").is_some() || attr("title").is_some() => "region",
            "dialog" => "dialog",
            "form" => "form",
            "table" => "table",
            "tr" => "row",
            "td" => "cell",
            "th" => "columnheader",
            "progress" => "progressbar",
            "meter" => "meter",
            "output" => "status",
            "hr" => "separator",
            "details" => "group",
            "fieldset" => "group",
            _ if attr("contenteditable").is_some_and(|v| v != "false") => "textbox",
            _ if self.clickable(n) => "clickable",
            _ => return None,
        };
        Some(role.into())
    }

    fn clickable(&self, n: &Node) -> bool {
        let pointer = |id: NodeId| self.e.doc.resolved_style_value(id, "cursor") == "pointer";
        if !pointer(n.id) {
            return false;
        }
        n.parent.is_none_or(|p| self.node(p).is_none_or(|p| !p.is_element() || !pointer(p.id)))
    }

    fn states(&self, n: &Node, tag: &str, attr: &dyn Fn(&str) -> Option<String>, role: Option<&str>) -> Vec<&'static str> {
        let mut states = Vec::new();
        let on = |name: &str| attr(name).is_some_and(|v| v == "true");
        if self.focus == Some(n.id) && !matches!(tag, "body" | "html") {
            states.push("focused");
        }
        let checkable = tag == "input" && matches!(role, Some("checkbox" | "radio"));
        if checkable && self.e.checked(n.id) || on("aria-checked") {
            states.push("checked");
        }
        if attr("aria-checked").as_deref() == Some("mixed") {
            states.push("mixed");
        }
        let option_selected = tag == "option" && self.e.selectedness(n.id);
        if on("aria-selected") || option_selected {
            states.push("selected");
        }
        match attr("aria-expanded").as_deref() {
            Some("true") => states.push("expanded"),
            Some("false") => states.push("collapsed"),
            _ if tag == "details" => states.push(if attr("open").is_some() { "expanded" } else { "collapsed" }),
            _ => {}
        }
        let form = matches!(tag, "button" | "input" | "select" | "textarea" | "option" | "fieldset");
        if on("aria-disabled") || form && attr("disabled").is_some() {
            states.push("disabled");
        }
        if on("aria-pressed") {
            states.push("pressed");
        }
        if attr("aria-current").is_some_and(|v| v != "false") {
            states.push("current");
        }
        if attr("readonly").is_some() || on("aria-readonly") {
            states.push("readonly");
        }
        if on("aria-modal") || tag == "dialog" && attr("open").is_some() {
            states.push("modal");
        }
        states
    }

    fn name(&self, n: &Node, tag: &str, attr: &dyn Fn(&str) -> Option<String>, content: bool) -> String {
        if let Some(label) = attr("aria-label").filter(|l| !l.trim().is_empty()) {
            return collapse(&label, NAME_MAX);
        }
        if let Some(ids) = attr("aria-labelledby") {
            let text: Vec<String> = ids.split_whitespace().filter_map(|id| self.e.doc.get_element_by_id(id)).map(|l| self.e.inner_text(l)).collect();
            let text = collapse(&text.join(" "), NAME_MAX);
            if !text.is_empty() {
                return text;
            }
        }
        if matches!(tag, "input" | "select" | "textarea" | "button" | "meter" | "output" | "progress")
            && let Some(label) = self.label_of(n, attr)
        {
            return label;
        }
        if matches!(tag, "input" | "select" | "textarea") {
            if let Some(p) = attr("placeholder").filter(|p| !p.trim().is_empty()) {
                return collapse(&p, NAME_MAX);
            }
            if matches!(attr("type").as_deref(), Some("button" | "submit" | "reset"))
                && let Some(v) = attr("value")
            {
                return collapse(&v, NAME_MAX);
            }
        }
        if tag == "img"
            && let Some(alt) = attr("alt")
        {
            return collapse(&alt, NAME_MAX);
        }
        if tag == "svg"
            && let Some(&title) = n.children.iter().find(|&&c| self.node(c).is_some_and(|c| c.element_data().is_some_and(|e| &*e.name.local == "title")))
        {
            return collapse(&self.e.inner_text(title), NAME_MAX);
        }
        if content {
            let text = collapse(&self.e.inner_text(n.id), NAME_MAX);
            if !text.is_empty() {
                return text;
            }
            if let Some(alt) = self.inner_label(n.id) {
                return alt;
            }
        }
        attr("title").map(|t| collapse(&t, NAME_MAX)).unwrap_or_default()
    }

    fn inner_label(&self, id: NodeId) -> Option<String> {
        for d in self.e.descendants(id) {
            let Some(e) = self.node(d).and_then(|n| n.element_data()) else { continue };
            for key in ["aria-label", "alt", "title"] {
                if let Some(v) = e.attrs().iter().find(|a| &*a.name.local == key).map(|a| a.value.trim().to_owned()).filter(|v| !v.is_empty()) {
                    return Some(collapse(&v, NAME_MAX));
                }
            }
        }
        None
    }

    fn label_of(&self, n: &Node, attr: &dyn Fn(&str) -> Option<String>) -> Option<String> {
        let by_for = attr("id").filter(|id| !id.is_empty()).and_then(|id| {
            self.e.descendants(self.e.doc.root_node().id).into_iter().find(|&d| {
                self.node(d)
                    .and_then(|d| d.element_data())
                    .is_some_and(|e| &*e.name.local == "label" && e.attrs().iter().any(|a| &*a.name.local == "for" && *a.value == id))
            })
        });
        let wrapping =
            || self.e.ancestors(n.id).into_iter().skip(1).find(|&a| self.node(a).and_then(|a| a.element_data()).is_some_and(|e| &*e.name.local == "label"));
        let label = by_for.or_else(wrapping)?;
        let text = collapse(&self.e.inner_text(label), NAME_MAX);
        (!text.is_empty()).then_some(text)
    }

    fn value(&self, id: NodeId, tag: &str, attr: &dyn Fn(&str) -> Option<String>, control: Control) -> Option<String> {
        if let Some(text) = attr("aria-valuetext").or_else(|| attr("aria-valuenow")) {
            return Some(collapse(&text, VALUE_MAX));
        }
        match control {
            Control::Editor(_) if attr("type").is_some_and(|t| t.eq_ignore_ascii_case("password")) => {
                let v = self.e.value(id);
                Some(if v.is_empty() { String::new() } else { "•••".into() })
            }
            Control::Editor(_) | Control::Value => Some(collapse(&self.e.value(id), VALUE_MAX)),
            Control::Select => {
                let option = self.e.options(id).into_iter().find(|&o| self.e.selectedness(o))?;
                Some(collapse(&option_label(self.e, option), VALUE_MAX))
            }
            _ if matches!(tag, "progress" | "meter") => attr("value"),
            _ if attr("contenteditable").is_some_and(|v| v != "false") => Some(collapse(&self.e.inner_text(id), VALUE_MAX)),
            _ => None,
        }
    }
}
