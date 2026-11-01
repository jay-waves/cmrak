//! Expand footnotes into native details beside their referring block.
use crate::assets::escape;
use pulldown_cmark::{Event, Tag, TagEnd};
use std::{collections::HashMap, ops::Range};
pub(crate) type Located = (Event<'static>, Range<usize>);
struct Owner {
    end: TagEnd,
    notes: Vec<(String, String, usize)>,
}

pub(crate) fn expand(body: &[Located], definitions: HashMap<String, Vec<Located>>) -> Vec<Located> {
    Notes {
        definitions,
        numbers: HashMap::new(),
        panels: 0,
        active: Vec::new(),
    }
    .expand(body)
}
struct Notes {
    definitions: HashMap<String, Vec<Located>>,
    numbers: HashMap<String, usize>,
    panels: usize,
    active: Vec<String>,
}
impl Notes {
    fn expand(&mut self, input: &[Located]) -> Vec<Located> {
        let mut output = Vec::new();
        let mut owners: Vec<Owner> = Vec::new();
        for (event, range) in input {
            match event {
                Event::Start(
                    tag @ (Tag::Paragraph | Tag::Item | Tag::Table(_) | Tag::Heading { .. }),
                ) => {
                    owners.push(Owner {
                        end: tag.to_end(),
                        notes: Vec::new(),
                    });
                    output.push((event.clone(), range.clone()));
                }
                Event::FootnoteReference(name) => {
                    let name = name.to_string();
                    let owner = owners
                        .iter()
                        .position(|owner| owner.end == TagEnd::Table)
                        .or_else(|| owners.len().checked_sub(1));
                    if !self.definitions.contains_key(&name)
                        || self.active.contains(&name)
                        || self.active.len() >= 32
                        || owner.is_none()
                    {
                        output.push((Event::Text(format!("[^{name}]").into()), range.clone()));
                        continue;
                    }
                    let first = !self.numbers.contains_key(&name);
                    let next = self.numbers.len() + 1;
                    let number = *self.numbers.entry(name.clone()).or_insert(next);
                    let owner = &mut owners[owner.unwrap()];
                    let id = if let Some((_, id, _)) = owner
                        .notes
                        .iter()
                        .find(|(existing, _, _)| existing == &name)
                    {
                        id.clone()
                    } else {
                        self.panels += 1;
                        let id = if first {
                            format!("F{number}")
                        } else {
                            format!("F{number}-{}", self.panels)
                        };
                        owner.notes.push((name, id.clone(), number));
                        id
                    };
                    output.push((Event::InlineHtml(format!("<sup class=\"footnote-ref\"><a href=\"#{id}\" data-note=\"{id}\" data-footnote-ref=\"{id}\" aria-controls=\"{id}\" aria-expanded=\"false\">[{number}]</a></sup>").into()),range.clone()));
                }
                Event::End(end) if owners.last().is_some_and(|owner| &owner.end == end) => {
                    // Tight list items have no paragraph: keep their panels inside li.
                    if *end != TagEnd::Item {
                        output.push((event.clone(), range.clone()));
                    }
                    let owner = owners.pop().unwrap();
                    for (name, id, number) in owner.notes {
                        let definition = self.definitions[&name].clone();
                        self.active.push(name);
                        output.push((Event::Html(format!("<details class=\"inline-footnote\" id=\"{}\"><summary>[{number}]</summary><div class=\"note-content\">\n",escape(&id)).into()),range.clone()));
                        output.extend(self.expand(&definition));
                        self.active.pop();
                        output.push((Event::Html("</div></details>\n".into()), range.clone()));
                    }
                    if *end == TagEnd::Item {
                        output.push((event.clone(), range.clone()));
                    }
                }
                _ => output.push((event.clone(), range.clone())),
            }
        }
        output
    }
}
