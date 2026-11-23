use blitz_dom::acss::properties::keywords::UserSelect;
use blitz_dom::{BaseDocument, Node};

pub(crate) fn of(doc: &BaseDocument) -> &'static str {
    let Some(node) = doc.get_hover_node_id().and_then(|id| doc.get_node(id)) else { return "default" };
    let Some(style) = ancestors(doc, node).find_map(Node::style) else { return "default" };
    match style.groups.cursor().keyword {
        "auto" if link(doc, node) => "pointer",
        "auto" if editable(doc, node) || (doc.hover_is_text() && selectable(doc, node)) => "text",
        "auto" => "default",
        keyword => keyword,
    }
}

fn ancestors<'a>(doc: &'a BaseDocument, node: &'a Node) -> impl Iterator<Item = &'a Node> {
    std::iter::successors(Some(node), |n| n.parent.and_then(|p| doc.get_node(p))).filter(|n| n.is_element())
}

fn selectable(doc: &BaseDocument, node: &Node) -> bool {
    ancestors(doc, node).filter_map(Node::style).map(|s| *s.groups.user_select()).find(|v| *v != UserSelect::Auto) != Some(UserSelect::None)
}

fn link(doc: &BaseDocument, node: &Node) -> bool {
    ancestors(doc, node).any(|n| n.element_data().is_some_and(|e| e.is_link()))
}

fn editable(doc: &BaseDocument, node: &Node) -> bool {
    ancestors(doc, node)
        .find_map(|n| {
            let element = n.element_data()?;
            if element.text_input_data().is_some() {
                return Some(!element.has_attr(blitz_dom::local_name!("disabled")));
            }
            element.attr(blitz_dom::local_name!("contenteditable")).map(|v| !v.eq_ignore_ascii_case("false"))
        })
        .unwrap_or(false)
}
