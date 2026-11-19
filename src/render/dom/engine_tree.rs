use super::*;

impl Engine {
    pub(crate) fn n(&self, id: u32) -> Result<NodeId> {
        let node = self.ids.node(id).map_err(|e| fail("NotFoundError", e))?;
        match self.doc.get_node(node) {
            Some(_) => Ok(node),
            None => Err(fail("NotFoundError", format!("node {id} is gone"))),
        }
    }

    pub(in crate::render::dom) fn node(&self, id: u32) -> Result<&Node> {
        let node = self.n(id)?;
        Ok(&self.doc.tree()[node])
    }

    pub(in crate::render::dom) fn element(&self, id: u32) -> Result<&blitz_dom::ElementData> {
        let n = self.node(id)?;
        match n.element_data() {
            Some(e) if !is_fragment(n) && n.is_element() => Ok(e),
            _ => Err(fail("InvalidNodeTypeError", format!("node {id} is not an element"))),
        }
    }

    pub(in crate::render::dom) fn fresh(&mut self, node: NodeId) -> u32 {
        if self.ids.xml_owned(None) && self.owner_of(node).is_none() {
            self.adopt_into(node, None);
        }
        self.ids.fresh.push(node);
        self.ids.id_of(node)
    }

    pub(in crate::render::dom) fn maybe(&self, id: u32) -> Result<Option<NodeId>> {
        match id {
            NONE => Ok(None),
            id => self.n(id).map(Some),
        }
    }

    pub(in crate::render::dom) fn out(&mut self, node: Option<NodeId>) -> u32 {
        node.map_or(NONE, |n| self.ids.id_of(n))
    }

    pub(in crate::render::dom) fn outs(&mut self, nodes: impl IntoIterator<Item = NodeId>) -> Vec<u32> {
        nodes.into_iter().map(|n| self.ids.id_of(n)).collect()
    }

    pub(crate) fn kind(&self, node: NodeId) -> NodeType {
        let Some(n) = self.doc.get_node(node) else { return NodeType::Comment };
        match &n.data {
            NodeData::Document(_) => NodeType::Document,
            NodeData::Element(_) if is_fragment(n) => match self.ids.documents.contains_key(&node) {
                true => NodeType::Document,
                false => NodeType::DocumentFragment,
            },
            NodeData::Element(_) | NodeData::AnonymousBlock(_) => NodeType::Element,
            NodeData::Text(_) => match self.ids.cdata.contains(&node) {
                true => NodeType::CDataSection,
                false => NodeType::Text,
            },
            NodeData::Comment { .. } if self.ids.doctypes.contains_key(&node) => NodeType::DocumentType,
            NodeData::Comment { .. } if self.ids.pis.contains_key(&node) => NodeType::ProcessingInstruction,
            NodeData::Comment { .. } => NodeType::Comment,
        }
    }

    pub(in crate::render::dom) fn sibling(&self, node: NodeId, step: isize) -> Option<NodeId> {
        let n = self.doc.get_node(node)?;
        let parent = self.doc.get_node(n.parent?)?;
        let index = parent.index_of_child(node)?.checked_add_signed(step)?;
        parent.children.get(index).copied()
    }

    pub(in crate::render::dom) fn connected(&self, node: NodeId) -> bool {
        let document = self.doc.root_node().id;
        let mut current = Some(node);
        while let Some(c) = current {
            if c == document {
                return true;
            }
            current = self.doc.get_node(c).and_then(|n| n.parent);
        }
        false
    }

    pub(in crate::render::dom) fn tree_root(&self, node: NodeId) -> NodeId {
        let mut node = node;
        loop {
            match self.doc.get_node(node).and_then(|n| n.parent) {
                Some(parent) => node = parent,
                None => match self.ids.templates.get(&node) {
                    Some(&template) if self.doc.get_node(template).is_some() => node = template,
                    _ => return node,
                },
            }
        }
    }

    pub(in crate::render::dom) fn subtree(&self, root: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            out.push(id);
            if let Some(content) = n.element_data().and_then(|e| e.template_contents) {
                stack.push(content);
            }
            stack.extend(n.children.iter().rev().copied());
        }
        out
    }

    pub(in crate::render::dom) fn descendants(&self, root: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let Some(r) = self.doc.get_node(root) else { return out };
        let mut stack: Vec<NodeId> = r.children.iter().rev().copied().collect();
        while let Some(id) = stack.pop() {
            let Some(n) = self.doc.get_node(id) else { continue };
            out.push(id);
            stack.extend(n.children.iter().rev().copied());
        }
        out
    }

    pub(in crate::render::dom) fn ancestors(&self, node: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut current = Some(node);
        while let Some(c) = current {
            out.push(c);
            current = self.doc.get_node(c).and_then(|n| n.parent);
        }
        out
    }

    pub(in crate::render::dom) fn compare(&self, a: NodeId, b: NodeId) -> u16 {
        const DISCONNECTED: u16 = 1;
        const PRECEDING: u16 = 2;
        const FOLLOWING: u16 = 4;
        const CONTAINS: u16 = 8;
        const CONTAINED_BY: u16 = 16;
        const IMPLEMENTATION_SPECIFIC: u16 = 32;
        if a == b {
            return 0;
        }
        let (chain_a, chain_b) = (self.ancestors(a), self.ancestors(b));
        if chain_a.last() != chain_b.last() {
            let order = if a < b { FOLLOWING } else { PRECEDING };
            return DISCONNECTED | IMPLEMENTATION_SPECIFIC | order;
        }
        if chain_b.contains(&a) {
            return CONTAINED_BY | FOLLOWING;
        }
        if chain_a.contains(&b) {
            return CONTAINS | PRECEDING;
        }
        let (mut ia, mut ib) = (chain_a.len(), chain_b.len());
        while ia > 0 && ib > 0 && chain_a[ia - 1] == chain_b[ib - 1] {
            ia -= 1;
            ib -= 1;
        }
        let common = chain_a[ia];
        let (branch_a, branch_b) = (chain_a[ia - 1], chain_b[ib - 1]);
        let parent = &self.doc.tree()[common];
        match parent.index_of_child(branch_a) < parent.index_of_child(branch_b) {
            true => FOLLOWING,
            false => PRECEDING,
        }
    }

    pub(in crate::render::dom) fn element_of(&self, hit: NodeId) -> Option<NodeId> {
        let mut node = self.doc.nearest_non_anonymous_ancestor(hit)?;
        loop {
            let n = self.doc.get_node(node)?;
            if n.is_element() {
                return Some(node);
            }
            node = n.parent?;
        }
    }

    pub(in crate::render::dom) fn new_fragment(&mut self) -> NodeId {
        self.tree().m.create_element(QualName::new(None, ns!(html), FRAGMENT.into()), Vec::new())
    }

    pub(in crate::render::dom) fn check_insert(&self, parent: NodeId, node: NodeId, child: Option<NodeId>) -> Result<()> {
        let hierarchy = |why: &str| Err(fail("HierarchyRequestError", why));
        let parent_kind = self.kind(parent);
        if !matches!(parent_kind, NodeType::Document | NodeType::DocumentFragment | NodeType::Element) {
            return hierarchy("this node type cannot have children");
        }
        let mut current = Some(parent);
        while let Some(c) = current {
            if c == node {
                return hierarchy("the new child is an ancestor of the parent");
            }
            current = self.doc.get_node(c).and_then(|n| n.parent).or_else(|| self.ids.templates.get(&c).copied());
        }
        if let Some(child) = child
            && self.doc.get_node(child).and_then(|n| n.parent) != Some(parent)
        {
            return Err(fail("NotFoundError", "the reference node is not a child of this node"));
        }
        let kind = self.kind(node);
        match kind {
            NodeType::Document => return hierarchy("a document cannot be inserted"),
            NodeType::Text | NodeType::CDataSection if parent_kind == NodeType::Document => return hierarchy("text cannot be a child of the document"),
            NodeType::DocumentType if parent_kind != NodeType::Document => {
                return hierarchy("a doctype can only be a child of the document");
            }
            _ => {}
        }
        if parent_kind != NodeType::Document {
            return Ok(());
        }
        let children = &self.doc.tree()[parent].children;
        let kind_of = |c: NodeId| self.kind(c);
        let has_element = children.iter().any(|&c| c != node && kind_of(c) == NodeType::Element);
        let has_doctype = children.iter().any(|&c| c != node && kind_of(c) == NodeType::DocumentType);
        let position = |c: NodeId| children.iter().position(|&x| x == c);
        let doctype_after = |c: NodeId| {
            let at = position(c).unwrap_or(children.len());
            children[at..].iter().any(|&x| kind_of(x) == NodeType::DocumentType)
        };
        let element_before = |c: NodeId| {
            let at = position(c).unwrap_or(0);
            children[..at].iter().any(|&x| kind_of(x) == NodeType::Element)
        };
        let child_is_doctype = child.is_some_and(|c| kind_of(c) == NodeType::DocumentType);
        match kind {
            NodeType::DocumentFragment => {
                let inner = &self.doc.tree()[node].children;
                let elements = inner.iter().filter(|&&c| kind_of(c) == NodeType::Element).count();
                if elements > 1 || inner.iter().any(|&c| matches!(kind_of(c), NodeType::Text | NodeType::CDataSection)) {
                    return hierarchy("a document has at most one element child and no text");
                }
                if elements == 1 && (has_element || child_is_doctype || child.is_some_and(doctype_after)) {
                    return hierarchy("a document has at most one element child, after the doctype");
                }
                Ok(())
            }
            NodeType::Element if has_element || child_is_doctype || child.is_some_and(doctype_after) => {
                hierarchy("a document has at most one element child, after the doctype")
            }
            NodeType::DocumentType if has_doctype || child.is_some_and(element_before) || (child.is_none() && has_element) => {
                hierarchy("a document has at most one doctype, before the element")
            }
            _ => Ok(()),
        }
    }

    pub(in crate::render::dom) fn expand(&self, node: NodeId) -> Vec<NodeId> {
        match self.kind(node) {
            NodeType::DocumentFragment => self.doc.tree()[node].children.to_vec(),
            _ => vec![node],
        }
    }

    pub(in crate::render::dom) fn insert(&mut self, parent: NodeId, child: NodeId, reference: Option<NodeId>) -> Result<()> {
        self.check_insert(parent, child, reference)?;
        let reference = match reference == Some(child) {
            true => self.sibling(child, 1),
            false => reference,
        };
        let nodes = self.expand(child);
        if nodes.is_empty() {
            return Ok(());
        }
        let owner = self.owner_of(parent);
        for &node in &nodes {
            if self.ids.owners.get(&node).copied() != owner {
                self.adopt_into(node, owner);
            }
        }
        let left = self.lists_left(&nodes);
        {
            let mut tree = self.tree();
            match reference {
                Some(reference) => tree.m.insert_nodes_before(reference, &nodes),
                None => tree.m.append_children(parent, &nodes),
            }
        }
        self.lists_changed(parent, &nodes, left);
        Ok(())
    }

    pub(in crate::render::dom) fn replace_children(&mut self, parent: NodeId, nodes: &[NodeId]) -> Result<()> {
        for &node in nodes {
            self.check_insert(parent, node, None).or_else(|e| {
                match self.kind(parent) == NodeType::Document && e.0.starts_with("HierarchyRequestError: a document has") {
                    true => Ok(()),
                    false => Err(e),
                }
            })?;
        }
        let new: Vec<NodeId> = nodes.iter().flat_map(|&n| self.expand(n)).collect();
        if self.kind(parent) == NodeType::Document {
            let count = |k: NodeType| new.iter().filter(|&&n| self.kind(n) == k).count();
            if count(NodeType::Element) > 1 || count(NodeType::DocumentType) > 1 || count(NodeType::Text) + count(NodeType::CDataSection) > 0 {
                return Err(fail("HierarchyRequestError", "a document has at most one element and one doctype child"));
            }
        }
        let owner = self.owner_of(parent);
        for &node in &new {
            if self.ids.owners.get(&node).copied() != owner {
                self.adopt_into(node, owner);
            }
        }
        let kept: std::collections::HashSet<NodeId> = new.iter().copied().collect();
        let old: Vec<NodeId> = self.doc.tree()[parent].children.iter().copied().filter(|c| !kept.contains(c)).collect();
        let left = self.lists_left(&new);
        {
            let mut tree = self.tree();
            for &c in &old {
                tree.m.remove_node(c);
            }
            if !new.is_empty() {
                tree.m.append_children(parent, &new);
            }
        }
        self.collect(&old);
        self.lists_changed(parent, &new, left);
        Ok(())
    }

    pub(in crate::render::dom) fn set_data(&mut self, node: NodeId, data: &str) {
        let is_text = self.doc.get_node(node).is_some_and(|n| n.is_text_node());
        if is_text {
            self.tree().m.set_node_text(node, data);
        } else if !self.ids.doctypes.contains_key(&node)
            && let Some(NodeData::Comment { contents }) = self.doc.get_node_mut(node).map(|n| &mut n.data)
        {
            *contents = data.to_owned();
        }
    }

    pub(in crate::render::dom) fn register(&mut self, root: NodeId) {
        for node in self.subtree(root) {
            let Some(e) = self.doc.get_node(node).and_then(|n| n.element_data()) else { continue };
            let html = e.name.ns == ns!(html);
            if html && &*e.name.local == "input" {
                self.show_form_state(node);
                continue;
            }
            if !(html && &*e.name.local == "img") {
                continue;
            }
            let id = self.ids.id_of(node);
            self.ids.images.entry(id).or_insert((String::new(), u8::MAX, 0));
        }
        self.settle_selects_in(root);
    }

    pub(in crate::render::dom) fn adopt(&mut self, parsed: crate::render::parse::Parsed) {
        self.ids.doctypes.extend(parsed.doctypes);
        self.ids.templates.extend(parsed.templates);
        self.ids.pis.extend(parsed.pis);
        self.ids.cdata.extend(parsed.cdata);
    }

    pub(in crate::render::dom) fn parse_document(&mut self, html: &str) {
        let document = self.doc.root_node().id;
        let old = self.doc.root_node().children.to_vec();
        self.ids.page = None;
        let parsed = {
            let mut tree = self.tree();
            for &c in &old {
                tree.m.remove_node(c);
            }
            crate::render::parse::document(&mut tree.m, document, html)
        };
        self.adopt(parsed);
        self.register(document);
        self.collect(&old);
    }

    pub(in crate::render::dom) fn parse_xml_document(&mut self, text: &str, content_type: &str) {
        let document = self.doc.root_node().id;
        let old = self.doc.root_node().children.to_vec();
        self.ids.page = Some(DocumentInfo::new("document", content_type, ""));
        let parsed = {
            let mut tree = self.tree();
            for &c in &old {
                tree.m.remove_node(c);
            }
            match crate::render::xml::document(&mut tree.m, document, text) {
                Ok(parsed) => parsed,
                Err(error) => {
                    for child in tree.m.child_ids(document) {
                        tree.m.remove_and_drop_node(child);
                    }
                    crate::render::xml::parsererror(&mut tree.m, document, &error);
                    crate::render::parse::Parsed::default()
                }
            }
        };
        self.adopt(parsed);
        self.adopt_into(document, None);
        self.register(document);
        self.collect(&old);
    }

    pub(in crate::render::dom) fn xml_scope(&self, context: NodeId) -> Vec<(String, String)> {
        let mut chain = vec![context];
        chain.extend(self.ancestors(context));
        let mut scope = Vec::new();
        for &node in chain.iter().rev() {
            let Some(e) = self.doc.get_node(node).and_then(|n| n.element_data()) else { continue };
            if is_fragment_name(&e.name) {
                continue;
            }
            scope.push((e.name.prefix.as_ref().map(|p| p.to_string()).unwrap_or_default(), e.name.ns.to_string()));
            for a in e.attrs() {
                if &*a.name.ns != crate::render::xml::XMLNS_NS {
                    continue;
                }
                match &a.name.prefix {
                    Some(_) => scope.push((a.name.local.to_string(), a.value.clone())),
                    None => scope.push((String::new(), a.value.clone())),
                }
            }
        }
        scope.retain(|(p, _)| p != "xml" && p != "xmlns");
        scope
    }

    pub(in crate::render::dom) fn parse_xml_fragment(&mut self, context: NodeId, text: &str) -> Result<NodeId> {
        let holder = self.new_fragment();
        let scope = self.xml_scope(context);
        let parsed = {
            let mut tree = self.tree();
            crate::render::xml::fragment(&mut tree.m, holder, text, scope)
        };
        match parsed {
            Ok(parsed) => {
                self.adopt(parsed);
                let owner = self.owner_of(context);
                self.adopt_into(holder, owner);
                self.register(holder);
                Ok(holder)
            }
            Err(error) => {
                self.tree().m.remove_and_drop_node(holder);
                Err(fail("SyntaxError", error))
            }
        }
    }

    pub(in crate::render::dom) fn parse_fragment(&mut self, context: NodeId, html: &str) -> NodeId {
        let holder = self.new_fragment();
        let element = self.doc.get_node(context).is_some_and(|n| n.is_element() && !is_fragment(n));
        let parsed = {
            let mut tree = self.tree();
            let body = (!element).then(|| tree.m.create_element(QualName::new(None, ns!(html), "body".into()), Vec::new()));
            let parsed = crate::render::parse::fragment(&mut tree.m, holder, body.unwrap_or(context), html);
            if let Some(body) = body {
                tree.m.remove_and_drop_node(body);
            }
            parsed
        };
        self.adopt(parsed);
        self.register(holder);
        holder
    }

    pub(in crate::render::dom) fn clone_node(&mut self, node: NodeId, deep: bool) -> Result<NodeId> {
        let copy = {
            let mut tree = self.tree();
            Self::clone_into(&mut tree, node, deep)?
        };
        self.register(copy);
        for n in self.subtree(copy) {
            self.show_form_state(n);
        }
        Ok(copy)
    }

    pub(in crate::render::dom) fn clone_into(tree: &mut Tree<'_>, node: NodeId, deep: bool) -> Result<NodeId> {
        enum What {
            Element(QualName, Vec<blitz_dom::Attribute>, Option<NodeId>),
            Text(String),
            Comment(String),
        }
        let n = tree.m.doc.get_node(node).ok_or_else(|| fail("NotFoundError", "node is gone"))?;
        let what = match &n.data {
            NodeData::Element(e) => What::Element(e.name.clone(), e.attrs().to_vec(), e.template_contents),
            NodeData::Text(t) => What::Text(t.content.clone()),
            NodeData::Comment { contents } => What::Comment(contents.clone()),
            NodeData::Document(_) | NodeData::AnonymousBlock(_) => {
                return Err(fail("NotSupportedError", "this node cannot be cloned"));
            }
        };
        let children = n.children.to_vec();
        let (copy, content) = match what {
            What::Element(name, attrs, content) => (tree.m.create_element(name, attrs), content),
            What::Text(text) => {
                let copy = tree.m.create_text_node(&text);
                if tree.ids.cdata.contains(&node) {
                    tree.ids.cdata.insert(copy);
                }
                (copy, None)
            }
            What::Comment(text) => {
                let copy = tree.m.create_comment_node(&text);
                if let Some(d) = tree.ids.doctypes.get(&node).cloned() {
                    tree.ids.doctypes.insert(copy, d);
                }
                if let Some(target) = tree.ids.pis.get(&node).cloned() {
                    tree.ids.pis.insert(copy, target);
                }
                (copy, None)
            }
        };
        if let Some(info) = tree.ids.documents.get(&node).cloned() {
            tree.ids.documents.insert(copy, info);
        } else {
            let owner = tree.ids.owners.get(&node).copied();
            if let Some(owner) = owner {
                tree.ids.owners.insert(copy, owner);
            }
            let xml = tree.ids.xml_owned(owner);
            if let Some(data) = tree.m.doc.get_node_mut(copy) {
                data.flags.set(blitz_dom::node::NodeFlags::IN_XML_DOCUMENT, xml);
            }
        }
        if let Some(form) = tree.ids.forms.get(&node).cloned() {
            tree.ids.forms.insert(copy, form);
        }
        if deep {
            for child in children {
                let c = Self::clone_into(tree, child, true)?;
                tree.m.append_children(copy, &[c]);
            }
            if let Some(content) = content {
                let target = tree.m.template_contents(copy);
                tree.ids.templates.insert(target, copy);
                let inner = tree.m.doc.get_node(content).map(|c| c.children.to_vec()).unwrap_or_default();
                for child in inner {
                    let c = Self::clone_into(tree, child, true)?;
                    tree.m.append_children(target, &[c]);
                }
            }
        }
        Ok(copy)
    }

    pub(crate) fn collect(&mut self, candidates: &[NodeId]) {
        let document = self.doc.root_node().id;
        let mut seen = std::collections::HashSet::new();
        let mut roots: Vec<NodeId> = Vec::new();
        for &c in candidates {
            if self.doc.get_node(c).is_none() {
                continue;
            }
            let root = self.tree_root(c);
            if root != document && seen.insert(root) && !self.holds_pin(root) {
                roots.push(root);
            }
        }
        for root in roots {
            self.free(root);
        }
    }

    pub(in crate::render::dom) fn holds_pin(&self, root: NodeId) -> bool {
        self.subtree(root).into_iter().any(|n| self.ids.pinned(n))
    }

    pub(in crate::render::dom) fn free(&mut self, root: NodeId) {
        let contents: Vec<NodeId> = self.subtree(root).into_iter().filter(|&n| self.ids.templates.contains_key(&n)).collect();
        if self.active.is_some_and(|a| self.tree_root(a) == root) {
            self.active = None;
        }
        let mut m = self.doc.mutate();
        let ids = &mut self.ids;
        for node in std::iter::once(root).chain(contents) {
            if m.doc.get_node(node).is_some() {
                m.remove_and_drop_node_with(node, &mut |dropped| ids.forget(dropped));
            }
        }
    }

    pub(in crate::render::dom) fn find_attribute(&self, node: NodeId, name: &str) -> Result<Option<(QualName, String)>> {
        let n = &self.doc.tree()[node];
        let e = n.element_data().filter(|_| n.is_element() && !is_fragment(n)).ok_or_else(|| fail("InvalidNodeTypeError", "not an element"))?;
        let name = match e.name.ns == ns!(html) && !self.in_xml_document(node) {
            true => std::borrow::Cow::Owned(name.to_ascii_lowercase()),
            false => std::borrow::Cow::Borrowed(name),
        };
        Ok(e.attrs()
            .iter()
            .find(|a| match &a.name.prefix {
                Some(prefix) => name.strip_prefix(&**prefix).and_then(|r| r.strip_prefix(':')) == Some(&*a.name.local),
                None => *a.name.local == *name,
            })
            .map(|a| (a.name.clone(), a.value.clone())))
    }

    pub(in crate::render::dom) fn find_attribute_ns(&self, node: NodeId, namespace: &str, local_name: &str) -> Result<Option<(QualName, String)>> {
        let n = &self.doc.tree()[node];
        let e = n.element_data().filter(|_| n.is_element() && !is_fragment(n)).ok_or_else(|| fail("InvalidNodeTypeError", "not an element"))?;
        Ok(e.attrs().iter().find(|a| *a.name.ns == *namespace && *a.name.local == *local_name).map(|a| (a.name.clone(), a.value.clone())))
    }

    pub(in crate::render::dom) fn store_attribute(&mut self, id: u32, node: NodeId, qual: QualName, value: Option<&str>) {
        let html = self.doc.tree()[node].element_data().is_some_and(|e| e.name.ns == ns!(html));
        let local = qual.local.clone();
        let plain = qual.ns == ns!();
        self.tree().set_attribute(id, node, qual, value);
        if !plain {
            return;
        }
        match &*local {
            "value" | "checked" | "min" | "max" | "step" | "type" => self.show_form_state(node),
            "src" => self.register(node),
            _ => {}
        }
        if html {
            self.select_attribute_changed(node, &local);
        }
    }

    pub(in crate::render::dom) fn xml_serialize(&self, node: NodeId, well_formed: bool, outer: bool) -> Result<String> {
        let documents = |n: NodeId| self.ids.documents.contains_key(&n);
        let serializer = crate::render::serialize::XmlSerializer { doc: &self.doc, ids: &self.ids, well_formed, documents: &documents };
        let result = match outer {
            true => serializer.outer(node),
            false => serializer.inner(node),
        };
        result.map_err(|message| fail("InvalidStateError", message))
    }

    pub(crate) fn owner_of(&self, node: NodeId) -> Option<NodeId> {
        match self.ids.documents.contains_key(&node) {
            true => Some(node),
            false => self.ids.owners.get(&node).copied(),
        }
    }

    pub(crate) fn in_xml_document(&self, node: NodeId) -> bool {
        match self.ids.owners.is_empty() && self.ids.documents.is_empty() {
            true => self.ids.xml_owned(None),
            false => self.ids.xml_owned(self.owner_of(node)),
        }
    }

    pub(in crate::render::dom) fn adopt_into(&mut self, node: NodeId, owner: Option<NodeId>) {
        let xml = self.ids.xml_owned(owner);
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if !self.ids.documents.contains_key(&n) {
                match owner {
                    Some(doc) => self.ids.owners.insert(n, doc),
                    None => self.ids.owners.remove(&n),
                };
            }
            if let Some(data) = self.doc.get_node_mut(n) {
                data.flags.set(blitz_dom::node::NodeFlags::IN_XML_DOCUMENT, xml);
            }
            if let Some(data) = self.doc.get_node(n) {
                stack.extend(data.children.iter().copied());
                if let Some(content) = data.element_data().and_then(|e| e.template_contents) {
                    stack.push(content);
                }
            }
        }
    }

    pub(in crate::render::dom) fn set_attribute_dom(&mut self, id: u32, name: &str, value: Option<&str>) -> Result<bool> {
        let node = self.n(id)?;
        let existing = self.find_attribute(node, name)?;
        let element = self.doc.tree()[node].element_data();
        let html = element.is_some_and(|e| e.name.ns == ns!(html)) && !self.in_xml_document(node);
        let canvas = html && element.is_some_and(|e| &*e.name.local == "canvas");
        let img = html && element.is_some_and(|e| &*e.name.local == "img");
        let again =
            |local: &str| matches!(local, "value" | "style") || (canvas && matches!(local, "width" | "height")) || (img && matches!(local, "src" | "srcset"));
        let qual = match existing {
            Some((qual, old)) if value == Some(&*old) && !again(&qual.local) => {
                return Ok(false);
            }
            Some((qual, _)) => qual,
            None if value.is_none() => return Ok(false),
            None => QualName::new(
                None,
                ns!(),
                match html {
                    true => name.to_ascii_lowercase().into(),
                    false => name.into(),
                },
            ),
        };
        self.store_attribute(id, node, qual, value);
        Ok(true)
    }
}
