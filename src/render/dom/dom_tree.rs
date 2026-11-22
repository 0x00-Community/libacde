use super::*;
use blitz_traits::net::Url;

impl Dom {
    pub fn document(&self) -> u32 {
        self.read(|e| {
            let document = e.doc.root_node().id;
            Ok(e.ids.id_of(document))
        })
        .unwrap_or(NONE)
    }

    pub fn parse_document(&self, html: &str) -> Result<()> {
        self.update(|e| e.parse_document(html))
    }

    pub fn set_base_url(&self, url: &str) -> Result<()> {
        let url = Url::parse(url).map_err(|e| Error::new(format!("base url {url}: {e}")))?;
        self.update(|e| e.doc.set_base_url(url.as_str()))
    }

    pub fn parse_xml_document(&self, text: &str, content_type: &str) -> Result<()> {
        self.update(|e| e.parse_xml_document(text, content_type))
    }

    pub fn viewport(&self) -> Viewport {
        self.read(|e| {
            let v = e.doc.viewport();
            let scale = v.scale_f64();
            Ok(Viewport { width: f64::from(v.window_size.0) / scale, height: f64::from(v.window_size.1) / scale, scale })
        })
        .unwrap_or(Viewport { width: 0.0, height: 0.0, scale: 1.0 })
    }

    pub fn resize(&self, width: u32, height: u32, scale: f32) {
        _ = self.update(|e| e.resize(width, height, scale));
    }

    pub fn set_dark(&self, on: bool) {
        _ = self.update(|e| e.set_dark(on));
    }

    pub fn node_type(&self, id: u32) -> Result<NodeType> {
        self.on(id, |e, n| e.kind(n))
    }

    pub fn parent(&self, id: u32) -> Result<u32> {
        self.read(|e| Ok(e.out(e.node(id)?.parent)))
    }

    pub fn ancestors(&self, id: u32) -> Result<Vec<u32>> {
        self.on(id, |e, n| e.outs(e.ancestors(n).into_iter().skip(1)))
    }

    pub fn first_child(&self, id: u32) -> Result<u32> {
        self.read(|e| Ok(e.out(e.node(id)?.children.first().copied())))
    }

    pub fn last_child(&self, id: u32) -> Result<u32> {
        self.read(|e| Ok(e.out(e.node(id)?.children.last().copied())))
    }

    pub fn next_sibling(&self, id: u32) -> Result<u32> {
        self.read(|e| Ok(e.out(e.sibling(e.n(id)?, 1))))
    }

    pub fn previous_sibling(&self, id: u32) -> Result<u32> {
        self.read(|e| Ok(e.out(e.sibling(e.n(id)?, -1))))
    }

    pub fn child_nodes(&self, id: u32) -> Result<Vec<u32>> {
        self.read(|e| Ok(e.outs(e.node(id)?.children.to_vec())))
    }

    pub fn children(&self, id: u32) -> Result<Vec<u32>> {
        self.read(|e| Ok(e.outs(e.node(id)?.children.iter().copied().filter(|&c| e.kind(c) == NodeType::Element).collect::<Vec<_>>())))
    }

    pub fn child_count(&self, id: u32) -> Result<u32> {
        self.read(|e| Ok(e.node(id)?.children.len() as u32))
    }

    pub fn is_connected(&self, id: u32) -> Result<bool> {
        self.on(id, |e, n| e.connected(n))
    }

    pub fn compare_document_position(&self, a: u32, b: u32) -> Result<u16> {
        self.read(|e| Ok(e.compare(e.n(a)?, e.n(b)?)))
    }

    pub fn create_element(&self, ns: Ns, local_name: &str) -> Result<u32> {
        if !valid_element_name(local_name) {
            return Err(fail("InvalidCharacterError", format!("'{local_name}' is not a valid element name")));
        }
        self.create(|e| Ok(e.tree().create_element(namespace(ns), local_name)))
    }

    pub fn create_element_ns(&self, namespace: &str, prefix: &str, local_name: &str) -> Result<u32> {
        self.create(|e| {
            let name = QualName::new((!prefix.is_empty()).then(|| prefix.into()), namespace.into(), local_name.into());
            Ok(e.tree().create_element_named(name))
        })
    }

    pub fn create_cdata(&self, data: &str) -> Result<u32> {
        self.create(|e| {
            let node = e.tree().m.create_text_node(data);
            e.ids.cdata.insert(node);
            Ok(node)
        })
    }

    pub fn create_processing_instruction(&self, target: &str, data: &str) -> Result<u32> {
        self.create(|e| {
            let node = e.tree().m.create_comment_node(data);
            e.ids.pis.insert(node, target.to_owned());
            Ok(node)
        })
    }

    pub fn processing_instruction_target(&self, id: u32) -> Result<String> {
        self.read(|e| e.ids.pis.get(&e.n(id)?).cloned().ok_or_else(|| fail("InvalidNodeTypeError", "not a processing instruction")))
    }

    pub fn create_document(&self, interface: &str, content_type: &str, url: &str) -> Result<u32> {
        self.create(|e| {
            let holder = e.new_fragment();
            e.ids.documents.insert(holder, DocumentInfo::new(interface, content_type, url));
            Ok(holder)
        })
    }

    pub fn document_info(&self, id: u32) -> Result<[String; 3]> {
        self.read(|e| {
            let node = e.n(id)?;
            let page = (node == e.doc.root_node().id).then_some(e.ids.page.as_ref()).flatten();
            Ok(match e.ids.documents.get(&node).or(page) {
                Some(info) => [info.interface.to_owned(), info.content_type.clone(), info.url.clone()],
                None => ["html".to_owned(), "text/html".to_owned(), String::new()],
            })
        })
    }

    pub fn owner_document(&self, id: u32) -> Result<u32> {
        self.read(|e| {
            let node = e.n(id)?;
            let owner = e.ids.owners.get(&node).copied();
            Ok(match owner {
                Some(doc) => e.ids.id_of(doc),
                None => e.ids.id_of(e.doc.root_node().id),
            })
        })
    }

    pub fn adopt(&self, id: u32, document: u32) -> Result<()> {
        self.write(|e| {
            let node = e.n(id)?;
            let document = e.n(document)?;
            let owner = (document != e.doc.root_node().id).then_some(document);
            e.adopt_into(node, owner);
            Ok(())
        })
    }

    pub fn in_html_document(&self, id: u32) -> Result<bool> {
        self.read(|e| Ok(!e.in_xml_document(e.n(id)?)))
    }

    pub fn parse_xml(&self, text: &str, content_type: &str, url: &str) -> Result<u32> {
        self.create(|e| {
            let holder = e.new_fragment();
            e.ids.documents.insert(holder, DocumentInfo::new("document", content_type, url));
            let parsed = {
                let mut tree = e.tree();
                match crate::render::xml::document(&mut tree.m, holder, text) {
                    Ok(parsed) => parsed,
                    Err(error) => {
                        for child in tree.m.child_ids(holder) {
                            tree.m.remove_and_drop_node(child);
                        }
                        crate::render::xml::parsererror(&mut tree.m, holder, &error);
                        crate::render::parse::Parsed::default()
                    }
                }
            };
            e.adopt(parsed);
            e.adopt_into(holder, Some(holder));
            e.register(holder);
            Ok(holder)
        })
    }

    pub fn namespace_uri(&self, id: u32) -> Result<String> {
        self.read(|e| Ok(e.element(id)?.name.ns.to_string()))
    }

    pub fn prefix(&self, id: u32) -> Result<String> {
        self.read(|e| Ok(e.element(id)?.name.prefix.as_ref().map(|p| p.to_string()).unwrap_or_default()))
    }

    pub fn attributes_ns(&self, id: u32) -> Result<Vec<String>> {
        self.read(|e| {
            Ok(e.element(id)?
                .attrs()
                .iter()
                .flat_map(|a| {
                    [a.name.ns.to_string(), a.name.prefix.as_ref().map(|p| p.to_string()).unwrap_or_default(), a.name.local.to_string(), a.value.clone()]
                })
                .collect())
        })
    }

    pub fn get_attribute_ns(&self, id: u32, namespace: &str, local_name: &str) -> Result<Option<String>> {
        self.read(|e| Ok(e.find_attribute_ns(e.n(id)?, namespace, local_name)?.map(|(_, v)| v)))
    }

    pub fn set_attribute_ns(&self, id: u32, namespace: &str, prefix: &str, local_name: &str, value: &str) -> Result<bool> {
        self.write_changed(&[id], |e| {
            let node = e.n(id)?;
            let qual = match e.find_attribute_ns(node, namespace, local_name)? {
                Some((qual, old)) if old == value && !matches!(&*qual.local, "value" | "style") => return Ok(false),
                Some((qual, _)) => qual,
                None => QualName::new((!prefix.is_empty()).then(|| prefix.into()), namespace.into(), local_name.into()),
            };
            e.store_attribute(id, node, qual, Some(value));
            Ok(true)
        })
    }

    pub fn remove_attribute_ns(&self, id: u32, namespace: &str, local_name: &str) -> Result<bool> {
        self.write_changed(&[id], |e| {
            let node = e.n(id)?;
            let Some((qual, _)) = e.find_attribute_ns(node, namespace, local_name)? else { return Ok(false) };
            e.store_attribute(id, node, qual, None);
            Ok(true)
        })
    }

    pub fn get_elements_by_tag_name_ns(&self, root: u32, namespace: &str, local_name: &str) -> Result<Vec<u32>> {
        self.elements(root, |el| (namespace == "*" || *el.name.ns == *namespace) && (local_name == "*" || *el.name.local == *local_name))
    }

    pub fn create_text(&self, data: &str) -> Result<u32> {
        self.create(|e| Ok(e.tree().m.create_text_node(data)))
    }

    pub fn create_comment(&self, data: &str) -> Result<u32> {
        self.create(|e| Ok(e.tree().m.create_comment_node(data)))
    }

    pub fn create_fragment(&self) -> Result<u32> {
        self.create(|e| Ok(e.new_fragment()))
    }

    pub fn create_document_type(&self, name: &str, public_id: &str, system_id: &str) -> Result<u32> {
        self.create(|e| {
            let node = e.tree().m.create_comment_node("");
            e.ids.doctypes.insert(node, Doctype { name: name.into(), public_id: public_id.into(), system_id: system_id.into() });
            Ok(node)
        })
    }

    pub fn clone(&self, id: u32, deep: bool) -> Result<u32> {
        self.create(|e| e.clone_node(e.n(id)?, deep))
    }

    pub fn insert_before(&self, parent: u32, child: u32, reference: u32) -> Result<()> {
        self.write_at(&[parent, child], |e| {
            let (parent, child, reference) = (e.n(parent)?, e.n(child)?, e.maybe(reference)?);
            e.insert(parent, child, reference)
        })
    }

    pub fn remove(&self, child: u32) -> Result<()> {
        self.edit(child, |e, node| {
            if let Some(parent) = e.doc.get_node(node).and_then(|n| n.parent) {
                let owner = e.list_owner(parent);
                e.tree().m.remove_node(node);
                e.collect(&[node]);
                if let Some(select) = owner {
                    e.settle_select(select, None);
                }
            }
        })
    }

    pub fn replace_children(&self, parent: u32, ids: &[u32]) -> Result<()> {
        let targets: Vec<u32> = std::iter::once(parent).chain(ids.iter().copied()).collect();
        self.write_at(&targets, |e| {
            let parent = e.n(parent)?;
            let nodes = ids.iter().map(|&id| e.n(id)).collect::<Result<Vec<_>>>()?;
            e.replace_children(parent, &nodes)
        })
    }

    pub fn local_name(&self, id: u32) -> Result<String> {
        self.read(|e| Ok(e.element(id)?.name.local.to_string()))
    }

    pub fn namespace(&self, id: u32) -> Result<Ns> {
        self.read(|e| Ok(ns_of(&e.element(id)?.name)))
    }

    pub fn get_attribute(&self, id: u32, name: &str) -> Result<Option<String>> {
        self.read(|e| Ok(e.find_attribute(e.n(id)?, name)?.map(|(_, v)| v)))
    }

    pub fn set_attribute(&self, id: u32, name: &str, value: &str) -> Result<bool> {
        if !valid_attribute_name(name) {
            return Err(fail("InvalidCharacterError", format!("'{name}' is not a valid attribute name")));
        }
        self.write_changed(&[id], |e| e.set_attribute_dom(id, name, Some(value)))
    }

    pub fn remove_attribute(&self, id: u32, name: &str) -> Result<bool> {
        self.write_changed(&[id], |e| e.set_attribute_dom(id, name, None))
    }

    pub fn has_attribute(&self, id: u32, name: &str) -> Result<bool> {
        self.read(|e| Ok(e.find_attribute(e.n(id)?, name)?.is_some()))
    }

    pub fn attributes(&self, id: u32) -> Result<Vec<String>> {
        self.read(|e| Ok(e.element(id)?.attrs().iter().flat_map(|a| [qualified(&a.name), a.value.clone()]).collect()))
    }

    pub fn id(&self, id: u32) -> Result<String> {
        self.read(|e| Ok(e.element(id)?.attr(blitz_dom::local_name!("id")).unwrap_or_default().to_owned()))
    }

    pub fn class_name(&self, id: u32) -> Result<String> {
        self.read(|e| Ok(e.element(id)?.attr(blitz_dom::local_name!("class")).unwrap_or_default().to_owned()))
    }

    pub fn document_type(&self, id: u32) -> Result<(String, String, String)> {
        self.read(|e| {
            let d = e.ids.doctypes.get(&e.n(id)?).ok_or_else(|| fail("InvalidNodeTypeError", "not a doctype"))?;
            Ok((d.name.clone(), d.public_id.clone(), d.system_id.clone()))
        })
    }

    pub fn data(&self, id: u32) -> Result<String> {
        self.read(|e| {
            let n = e.node(id)?;
            Ok(match &n.data {
                NodeData::Text(t) => t.content.clone(),
                NodeData::Comment { contents } if !e.ids.doctypes.contains_key(&n.id) => contents.clone(),
                _ => String::new(),
            })
        })
    }

    pub fn set_data(&self, id: u32, data: &str) -> Result<()> {
        self.edit(id, |e, n| e.set_data(n, data))
    }

    pub fn text_content(&self, id: u32) -> Result<String> {
        self.read(|e| {
            let n = e.node(id)?;
            Ok(match &n.data {
                NodeData::Document(_) => String::new(),
                NodeData::Comment { contents } if !e.ids.doctypes.contains_key(&n.id) => contents.clone(),
                NodeData::Comment { .. } => String::new(),
                _ => n.text_content(),
            })
        })
    }

    pub fn set_text_content(&self, id: u32, text: &str) -> Result<()> {
        self.edit(id, |e, node| match e.kind(node) {
            NodeType::Element | NodeType::DocumentFragment => {
                let text = (!text.is_empty()).then(|| e.tree().m.create_text_node(text));
                let nodes: Vec<NodeId> = text.into_iter().collect();
                e.replace_children(node, &nodes)
            }
            NodeType::Text | NodeType::CDataSection | NodeType::Comment | NodeType::ProcessingInstruction => {
                e.set_data(node, text);
                Ok(())
            }
            NodeType::Document | NodeType::DocumentType => Ok(()),
        })
        .flatten()
    }

    pub fn inner_text(&self, id: u32) -> Result<String> {
        self.on(id, |e, n| {
            e.resolved();
            e.inner_text(n)
        })
    }

    pub fn inner_html(&self, id: u32) -> Result<String> {
        self.read(|e| {
            let n = e.n(id)?;
            match e.in_xml_document(n) {
                true => e.xml_serialize(n, true, false),
                false => Ok(Serializer { doc: &e.doc, ids: &e.ids }.inner(n)),
            }
        })
    }

    pub fn outer_html(&self, id: u32) -> Result<String> {
        self.read(|e| {
            let n = e.n(id)?;
            match e.in_xml_document(n) {
                true => e.xml_serialize(n, true, true),
                false => Ok(Serializer { doc: &e.doc, ids: &e.ids }.outer(n)),
            }
        })
    }

    pub fn serialize_xml(&self, id: u32) -> Result<String> {
        self.read(|e| {
            let n = e.n(id)?;
            e.xml_serialize(n, false, true)
        })
    }

    pub fn parse_fragment(&self, context: u32, html: &str) -> Result<u32> {
        self.create(|e| {
            let context = e.n(context)?;
            if e.in_xml_document(context) {
                return e.parse_xml_fragment(context, html);
            }
            let holder = e.parse_fragment(context, html);
            let owner = e.owner_of(context);
            if owner.is_some() {
                e.adopt_into(holder, owner);
            }
            Ok(holder)
        })
    }

    pub fn template_content(&self, id: u32) -> Result<u32> {
        self.edit(id, |e, node| {
            e.element(id)?;
            let content = e.tree().m.template_contents(node);
            e.ids.templates.insert(content, node);
            Ok(e.ids.id_of(content))
        })
        .flatten()
    }

    pub fn query_selector(&self, root: u32, selector: &str) -> Result<u32> {
        self.read(|e| {
            let found = e.doc.query_selector_in(e.n(root)?, selector).map_err(|_| bad_selector(selector))?;
            Ok(e.out(found))
        })
    }

    pub fn query_selector_all(&self, root: u32, selector: &str) -> Result<Vec<u32>> {
        self.read(|e| {
            let found = e.doc.query_selector_all_in(e.n(root)?, selector).map_err(|_| bad_selector(selector))?;
            Ok(e.outs(found))
        })
    }

    pub fn matches(&self, id: u32, selector: &str) -> Result<bool> {
        self.read(|e| e.doc.matches_selector(e.n(id)?, selector).map_err(|_| bad_selector(selector)))
    }

    pub fn closest(&self, id: u32, selector: &str) -> Result<u32> {
        self.read(|e| {
            let found = e.doc.closest(e.n(id)?, selector).map_err(|_| bad_selector(selector))?;
            Ok(e.out(found))
        })
    }

    pub fn get_element_by_id(&self, id: &str) -> u32 {
        self.read(|e| Ok(e.out(e.doc.get_element_by_id(id)))).unwrap_or(NONE)
    }

    pub fn get_elements_by_class_name(&self, root: u32, class_names: &str) -> Result<Vec<u32>> {
        let wanted: Vec<&str> = class_names.split_ascii_whitespace().collect();
        self.elements(root, |el| {
            let classes = el.attr(blitz_dom::local_name!("class")).unwrap_or_default();
            !wanted.is_empty() && wanted.iter().all(|w| classes.split_ascii_whitespace().any(|c| c == *w))
        })
    }

    pub fn get_elements_by_tag_name(&self, root: u32, qualified_name: &str) -> Result<Vec<u32>> {
        let lower = qualified_name.to_ascii_lowercase();
        let xml = self.read(|e| Ok(e.in_xml_document(e.n(root)?)))?;
        self.elements(root, |el| qualified_name == "*" || qualified(&el.name) == if el.name.ns == ns!(html) && !xml { lower.as_str() } else { qualified_name })
    }

    pub(in crate::render::dom) fn elements(&self, root: u32, keep: impl Fn(&blitz_dom::ElementData) -> bool) -> Result<Vec<u32>> {
        self.on(root, |e, root| {
            let found: Vec<NodeId> = e
                .descendants(root)
                .into_iter()
                .filter(|&d| e.doc.get_node(d).and_then(|n| n.element_data()).is_some_and(|el| !is_fragment_name(&el.name) && keep(el)))
                .collect();
            e.outs(found)
        })
    }
}
