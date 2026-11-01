use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct IdState {
    counts: HashMap<String, usize>,
    used: HashSet<String>,
}

impl IdState {
    pub(super) fn next(&mut self, base: String) -> String {
        let occurrence = self.counts.entry(base.clone()).or_default();
        loop {
            let id = if *occurrence == 0 {
                base.clone()
            } else {
                format!("{base}-{occurrence}")
            };
            *occurrence += 1;
            let section = format!("section-{id}");
            if !self.used.contains(&id) && !self.used.contains(&section) {
                self.used.insert(id.clone());
                self.used.insert(section);
                return id;
            }
        }
    }
}

pub(super) fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for ch in text.trim().to_lowercase().chars() {
        if ch.is_whitespace() {
            pending_dash = !slug.is_empty();
        } else if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            if pending_dash && !slug.ends_with('-') {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch);
        }
    }
    if slug.is_empty() {
        "heading".into()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_suffixes_do_not_collide_with_heading_text() {
        for headings in [
            ["foo", "foo", "foo-1", "foo"],
            ["foo-1", "foo", "foo", "foo-1"],
        ] {
            let mut state = IdState::default();
            let ids: HashSet<_> = headings
                .into_iter()
                .map(|text| state.next(text.into()))
                .collect();
            assert_eq!(ids.len(), headings.len());
        }
    }
}
