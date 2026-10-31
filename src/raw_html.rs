//! Rewrite resource attributes without serializing or otherwise changing raw HTML.
use crate::{assets::escape, document::Attachments};

pub fn rewrite(html: &str, attachments: &Attachments) -> Result<String, String> {
    let bytes = html.as_bytes();
    let mut result = String::with_capacity(html.len());
    let mut cursor = 0;
    while let Some(offset) = html[cursor..].find('<') {
        let start = cursor + offset;
        result.push_str(&html[cursor..start]);
        if html[start..].starts_with("<!--") {
            let end = html[start + 4..]
                .find("-->")
                .map_or(html.len(), |offset| start + 4 + offset + 3);
            result.push_str(&html[start..end]);
            cursor = end;
            continue;
        }
        let mut at = start + 1;
        while bytes.get(at).is_some_and(u8::is_ascii_alphanumeric) {
            at += 1;
        }
        if at == start + 1 {
            result.push('<');
            cursor = start + 1;
            continue;
        }
        let tag = html[start + 1..at].to_ascii_lowercase();
        result.push_str(&html[start..at]);
        let mut unchanged = at;
        while at < bytes.len() {
            while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            if bytes.get(at).is_none_or(|byte| matches!(byte, b'>' | b'/')) {
                break;
            }
            let name_start = at;
            while bytes.get(at).is_some_and(|byte| {
                !byte.is_ascii_whitespace() && !matches!(byte, b'=' | b'>' | b'/')
            }) {
                at += 1;
            }
            let name = html[name_start..at].to_ascii_lowercase();
            while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            if bytes.get(at) != Some(&b'=') {
                continue;
            }
            at += 1;
            while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
                at += 1;
            }
            let quote = bytes
                .get(at)
                .copied()
                .filter(|byte| matches!(byte, b'\'' | b'"'));
            let value_start = at;
            if quote.is_some() {
                at += 1;
            }
            let content_start = at;
            while bytes.get(at).is_some_and(|byte| match quote {
                Some(quote) => *byte != quote,
                None => !byte.is_ascii_whitespace() && *byte != b'>',
            }) {
                at += 1;
            }
            let value = &html[content_start..at];
            if quote.is_some() && at < bytes.len() {
                at += 1;
            }
            let image = (tag == "img" && name == "src")
                || (tag == "video" && name == "poster")
                || (tag == "image" && matches!(name.as_str(), "href" | "xlink:href"));
            let rewritten = if matches!(name.as_str(), "srcset" | "imagesrcset") {
                Some(rewrite_srcset(&decode_entities(value), attachments)?)
            } else if matches!(name.as_str(), "src" | "href" | "xlink:href" | "poster")
                || (tag == "object" && name == "data")
            {
                Some(attachments.resolve(&decode_entities(value), image)?)
            } else {
                None
            };
            if let Some(value) = rewritten {
                result.push_str(&html[unchanged..value_start]);
                result.push('"');
                result.push_str(&escape(&value));
                result.push('"');
                unchanged = at;
            }
        }
        // Keep raw text intact; strings inside CSS or text elements are not HTML tags.
        let end = html[at..]
            .find('>')
            .map_or(html.len(), |offset| at + offset + 1);
        result.push_str(&html[unchanged..end]);
        cursor = end;
        if matches!(tag.as_str(), "style" | "textarea" | "title" | "xmp") {
            let close = format!("</{tag}");
            let end = bytes[cursor..]
                .windows(close.len())
                .position(|window| window.eq_ignore_ascii_case(close.as_bytes()))
                .map_or(html.len(), |offset| cursor + offset);
            result.push_str(&html[cursor..end]);
            cursor = end;
        }
    }
    result.push_str(&html[cursor..]);
    Ok(result)
}

fn decode_entities(value: &str) -> String {
    let mut result = String::new();
    let mut cursor = 0;
    while let Some(offset) = value[cursor..].find('&') {
        let start = cursor + offset;
        result.push_str(&value[cursor..start]);
        if let Some(offset) = value[start..].find(';') {
            let end = start + offset + 1;
            if let Ok(decoded) = quick_xml::escape::unescape(&value[start..end]) {
                result.push_str(&decoded);
                cursor = end;
                continue;
            }
        }
        result.push('&');
        cursor = start + 1;
    }
    result.push_str(&value[cursor..]);
    result
}

fn rewrite_srcset(value: &str, attachments: &Attachments) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut result = String::new();
    let mut at = 0;
    while at < bytes.len() {
        let separator = at;
        while bytes
            .get(at)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b',')
        {
            at += 1;
        }
        result.push_str(&value[separator..at]);
        let start = at;
        while bytes
            .get(at)
            .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            at += 1;
        }
        let url = value[start..at].trim_end_matches(',');
        result.push_str(&attachments.resolve(url, true)?);
        result.push_str(&value[start + url.len()..at]);
        if url.len() == at - start {
            let descriptor = at;
            while bytes.get(at).is_some_and(|byte| *byte != b',') {
                at += 1;
            }
            result.push_str(&value[descriptor..at]);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Images;
    use std::fs;

    #[test]
    fn html_resources_resolve_against_source_and_respect_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(temp.path()).unwrap();
        fs::create_dir(root.join("notes")).unwrap();
        fs::write(root.join("a & b.png"), b"png").unwrap();
        let attachments = Attachments::new(
            &root.join("notes/note.md"),
            &root.join("temporary/note.html"),
            root.clone(),
            Images::Reference,
            false,
        )
        .unwrap();
        let image = crate::assets::file_url(&root.join("a & b.png")).unwrap();
        let html = rewrite(
            "<!-- <img src='missing.png'> --><figure title='x > y'><IMG SRC='../a &amp; b.png' srcset='../a%20%26%20b.png 1x, data:image/png;base64,YQ== 2x'></figure><video src=../clip.mp4 poster='../a &amp; b.png'></video><a href='../manual.pdf?x=1&amp;y=2#part'>PDF</a><a href='#local'>Jump</a><link href='../style.css'><style>.x::before{content:'<img src=missing.png>'}</style>",
            &attachments,
        )
        .unwrap();
        assert_eq!(
            html.matches(&format!("\"{}\"", escape(image.as_str())))
                .count(),
            2
        );
        assert!(html.contains(&format!(
            "srcset=\"{} 1x, data:image/png;base64,YQ== 2x\"",
            escape(image.as_str())
        )));
        for (attribute, path) in [("src", "clip.mp4"), ("href", "style.css")] {
            let url = crate::assets::file_url(&root.join(path)).unwrap();
            assert!(html.contains(&format!("{attribute}=\"{url}\"")));
        }
        let manual = crate::assets::file_url(&root.join("manual.pdf")).unwrap();
        assert!(html.contains(&format!("href=\"{manual}?x=1&amp;y=2#part\"")));
        assert!(html.contains("href=\"#local\""));
        assert!(html.contains("<!-- <img src='missing.png'> -->"));
        assert!(html.contains("content:'<img src=missing.png>'"));
        assert!(rewrite("<img src='missing.png'>", &attachments).is_err());
        assert!(
            rewrite("<a href='../../outside.pdf'>outside</a>", &attachments)
                .unwrap_err()
                .contains("outside root")
        );
    }
}
