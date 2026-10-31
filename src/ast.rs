//! AST transformations shared by the Markdown rendering pipeline.

use super::slugify;
use comrak::{
    Arena, Options,
    nodes::{Ast, AstNode, NodeValue},
};
use std::collections::HashMap;

type FootnotePanel = (String, String, String, u32);
type FootnoteGroup<'a> = (&'a AstNode<'a>, Vec<FootnotePanel>);

type Definitions<'a> = HashMap<String, &'a AstNode<'a>>;

fn add_inline_footnotes<'a>(
    arena: &'a Arena<'a>,
    root: &'a AstNode<'a>,
    options: &Options<'_>,
) -> Result<(), String> {
    let definitions = root
        .descendants()
        .filter_map(|node| {
            if let NodeValue::FootnoteDefinition(definition) = &node.data().value {
                Some((definition.name.clone(), node))
            } else {
                None
            }
        })
        .collect::<Definitions<'_>>();
    for definition in definitions.values() {
        definition.detach();
    }
    expand_footnotes(arena, root, &definitions, options, "", &[])
}

// Clone the parsed body rather than parsing its text again: nested Markdown,
// math and raw-HTML policy continue to use the same Comrak rendering pipeline.
fn clone_body<'a>(arena: &'a Arena<'a>, node: &'a AstNode<'a>) -> &'a AstNode<'a> {
    let mut data = node.data().clone();
    data.sourcepos = (0, 0, 0, 0).into();
    let copy = arena.alloc(data.into());
    for child in node.children() {
        copy.append(clone_body(arena, child));
    }
    copy
}

fn expand_footnotes<'a>(
    arena: &'a Arena<'a>,
    root: &'a AstNode<'a>,
    definitions: &Definitions<'a>,
    options: &Options<'_>,
    prefix: &str,
    ancestors: &[String],
) -> Result<(), String> {
    // Collect before mutation. A table owns all references in its cells;
    // otherwise the nearest paragraph/heading owns its panels.
    let references = root
        .descendants()
        .filter(|node| matches!(node.data().value, NodeValue::FootnoteReference(_)))
        .collect::<Vec<_>>();
    let mut groups: Vec<FootnoteGroup<'_>> = Vec::new();
    let mut occurrences = HashMap::<String, usize>::new();
    for reference in references {
        let NodeValue::FootnoteReference(note) = reference.data().value.clone() else {
            continue;
        };
        let Some(_) = definitions.get(&note.name) else {
            continue;
        };
        if ancestors.len() >= 32 || ancestors.contains(&note.name) {
            // Cyclic definitions must not recursively allocate panels forever.
            reference.data.borrow_mut().value = NodeValue::Text(format!("[{}]", note.ix).into());
            continue;
        }
        let container = reference
            .ancestors()
            .find(|node| matches!(node.data().value, NodeValue::Table(_)))
            .or_else(|| {
                reference.ancestors().find(|node| {
                    matches!(
                        node.data().value,
                        NodeValue::Paragraph | NodeValue::Heading(_)
                    )
                })
            });
        let Some(container) = container else { continue };
        let occurrence = occurrences.entry(note.name.clone()).or_default();
        let suffix = if *occurrence == 0 {
            String::new()
        } else {
            format!("-{}", *occurrence)
        };
        *occurrence += 1;
        let reference_id = format!("{prefix}fnref{}{suffix}", note.ix);
        let group_index = groups
            .iter()
            .position(|(node, _)| std::ptr::eq(*node, container))
            .unwrap_or_else(|| {
                groups.push((container, Vec::new()));
                groups.len() - 1
            });
        let panels = &mut groups[group_index].1;
        let target = if let Some((_, target, _, _)) =
            panels.iter().find(|(name, _, _, _)| *name == note.name)
        {
            target.clone()
        } else {
            let target = format!("{prefix}F{}{suffix}", note.ix);
            panels.push((note.name, target.clone(), reference_id.clone(), note.ix));
            target
        };
        reference.data.borrow_mut().value = NodeValue::Raw(format!(
            "<sup class=\"footnote-ref\"><a id=\"{reference_id}\" href=\"#{target}\" data-footnote-ref=\"{target}\" aria-controls=\"{target}\" aria-expanded=\"false\">[{}]</a></sup>",
            note.ix
        ));
    }
    for (container, panels) in groups {
        let mut insertion = container;
        for (name, target, reference_id, number) in panels {
            let body = arena.alloc(Ast::new(NodeValue::Document, Default::default()).into());
            for child in definitions[&name].children() {
                body.append(clone_body(arena, child));
            }
            let mut chain = ancestors.to_vec();
            chain.push(name);
            expand_footnotes(
                arena,
                body,
                definitions,
                options,
                &format!("{target}-"),
                &chain,
            )?;
            let label = raw(
                arena,
                format!(
                    "<a href=\"#{reference_id}\" class=\"footnote-label\" aria-label=\"Close footnote and return to reference\">[{number}]</a>"
                ),
            );
            if let Some(first) = body
                .first_child()
                .filter(|node| matches!(node.data().value, NodeValue::Paragraph))
            {
                first.prepend(raw(arena, " ".into()));
                first.prepend(label);
            } else {
                let paragraph =
                    arena.alloc(Ast::new(NodeValue::Paragraph, Default::default()).into());
                paragraph.append(label);
                body.prepend(paragraph);
            }
            let mut body_options = options.clone();
            body_options.render.sourcepos = false;
            let mut html = String::new();
            comrak::format_html(body, &body_options, &mut html)
                .map_err(|error| error.to_string())?;
            let panel = raw(
                arena,
                format!(
                    "<aside id=\"{target}\" class=\"inline-footnote\" role=\"doc-footnote\" hidden>\n{html}</aside>\n"
                ),
            );
            insertion.insert_after(panel);
            insertion = panel;
        }
    }
    Ok(())
}

pub(super) fn transform<'a>(
    arena: &'a Arena<'a>,
    root: &'a AstNode<'a>,
    options: &Options<'_>,
) -> Result<(), String> {
    add_inline_footnotes(arena, root, options)?;
    add_heading_sections(arena, root);
    Ok(())
}

fn raw<'a>(arena: &'a Arena<'a>, html: String) -> &'a AstNode<'a> {
    arena.alloc(Ast::new(NodeValue::Raw(html), Default::default()).into())
}

fn add_heading_sections<'a>(arena: &'a Arena<'a>, root: &'a AstNode<'a>) {
    add_heading_sections_with_native_details(arena, root, false);
}

pub(crate) fn add_static_heading_sections<'a>(arena: &'a Arena<'a>, root: &'a AstNode<'a>) {
    add_heading_sections_with_native_details(arena, root, true);
}

fn add_heading_sections_with_native_details<'a>(
    arena: &'a Arena<'a>,
    root: &'a AstNode<'a>,
    native_details: bool,
) {
    let mut top = Vec::new();
    let mut node = root.first_child();
    while let Some(current) = node {
        top.push(current);
        node = current.next_sibling();
    }
    let mut stack: Vec<(&'a AstNode<'a>, u8, &'a AstNode<'a>, bool)> = Vec::new();
    let mut ids = super::headings::IdState::default();
    let heading_ids: HashMap<_, _> = root
        .descendants()
        .filter(|node| matches!(node.data().value, NodeValue::Heading(_)))
        .map(|node| (node as *const _, ids.next(slugify(&node.collect_text()))))
        .collect();

    for current in top {
        let heading_level = match current.data().value {
            NodeValue::Heading(heading) => Some(heading.level),
            _ => None,
        };
        if let Some(level) = heading_level {
            if let Some((_, _, _, has_content)) = stack.last_mut() {
                *has_content = true;
            }
            if stack.last().is_some() {
                current.insert_before(raw(arena, "</div>\n".into()));
            }
            while stack
                .last()
                .is_some_and(|(_, parent_level, _, _)| *parent_level >= level)
            {
                let (_, _, open, has_content) = stack.pop().unwrap();
                if let NodeValue::Raw(html) = &mut open.data.borrow_mut().value
                    && has_content
                    && !html.contains("has-heading-content")
                {
                    *html = html.replacen(
                        "class=\"heading-section\"",
                        "class=\"heading-section has-heading-content\"",
                        1,
                    );
                }
                current.insert_before(raw(
                    arena,
                    if native_details {
                        "</details>\n"
                    } else {
                        "</section>\n"
                    }
                    .into(),
                ));
            }
            let heading_id = &heading_ids[&(current as *const _)];
            let open = raw(
                arena,
                if native_details {
                    format!(
                        "<details class=\"heading-section\" data-heading-level=\"{level}\" id=\"section-{heading_id}\" open>\n"
                    )
                } else {
                    format!(
                        "<section class=\"heading-section\" data-heading-level=\"{level}\" id=\"section-{heading_id}\">\n"
                    )
                },
            );
            current.insert_before(open);
            current.insert_before(raw(arena, if native_details {
                "<summary class=\"heading-summary\">\n".into()
            } else {
                "<div class=\"heading-summary\" role=\"button\" tabindex=\"0\" aria-expanded=\"true\">\n".into()
            }));
            current.insert_after(raw(arena, if native_details {
                "</summary>\n<div class=\"heading-content\">\n".into()
            } else {
                "<span class=\"heading-collapsed-indicator\" aria-hidden=\"true\">…</span></div>\n<div class=\"heading-content\">\n".into()
            }));
            stack.push((current, level, open, false));
        } else if let Some((_, _, _, has_content)) = stack.last_mut() {
            *has_content = true;
        }
    }

    while let Some((_, _, open, has_content)) = stack.pop() {
        if let NodeValue::Raw(html) = &mut open.data.borrow_mut().value
            && has_content
            && !html.contains("has-heading-content")
        {
            *html = html.replacen(
                "class=\"heading-section\"",
                "class=\"heading-section has-heading-content\"",
                1,
            );
        }
        root.append(raw(
            arena,
            if native_details {
                "</div>\n</details>\n"
            } else {
                "</div>\n</section>\n"
            }
            .into(),
        ));
    }
}
