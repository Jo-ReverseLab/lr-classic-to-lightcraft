//! Reading the title from the XMP blobs in `Adobe_AdditionalMetadata.xmp`.
//!
//! A blob is 4 header bytes followed by a zlib stream of the XMP packet. The
//! header is not used. Only a small cut-out of XMP is understood: the
//! `rdf:li` entries inside `dc:title`.

use flate2::read::ZlibDecoder;
use std::io::Read;

/// Upper bound for an unpacked XMP packet; larger packets are treated as
/// unreadable (protection against zip bombs).
const MAX_XMP_BYTES: u64 = 16 * 1024 * 1024;

/// Length of the header in front of the zlib stream.
const HEADER_BYTES: usize = 4;

/// The blob could not be turned into XMP text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unreadable;

/// Unpacks a blob into XMP text.
///
/// Fails if the blob is shorter than the header, is not a zlib stream,
/// unpacks to more than [`MAX_XMP_BYTES`] or is not UTF-8.
pub(crate) fn unpack(blob: &[u8]) -> Result<String, Unreadable> {
    let data = blob.get(HEADER_BYTES..).ok_or(Unreadable)?;
    let mut out = Vec::new();
    ZlibDecoder::new(data)
        .take(MAX_XMP_BYTES + 1)
        .read_to_end(&mut out)
        .map_err(|_| Unreadable)?;
    if out.len() as u64 > MAX_XMP_BYTES {
        return Err(Unreadable);
    }
    String::from_utf8(out).map_err(|_| Unreadable)
}

/// The title of a blob: `Ok(None)` if the XMP has no usable title.
pub(crate) fn title_from_blob(blob: &[u8]) -> Result<Option<String>, Unreadable> {
    Ok(title_from_xmp(&unpack(blob)?))
}

/// Extracts `dc:title` from XMP text.
///
/// The `rdf:li` with `xml:lang="x-default"` wins, else the first `rdf:li`.
/// Entities are decoded; the text is otherwise unchanged. An empty or
/// whitespace-only title gives `None`.
pub(crate) fn title_from_xmp(xmp: &str) -> Option<String> {
    let body = element_body(xmp, "dc:title")?;
    let mut first = None;
    let mut chosen = None;
    let mut rest = body;
    while let Some((attrs, text, after)) = next_element(rest, "rdf:li") {
        if first.is_none() {
            first = Some(text);
        }
        if is_x_default(attrs) {
            chosen = Some(text);
            break;
        }
        rest = after;
    }
    let text = decode_entities(chosen.or(first)?);
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Finds the first start tag `<name` (followed by whitespace, `>` or `/`)
/// at or after `from`; returns the index of `<`.
fn find_start_tag(s: &str, name: &str, from: usize) -> Option<usize> {
    let open = format!("<{name}");
    let mut pos = from;
    while let Some(i) = s[pos..].find(&open) {
        let start = pos + i;
        let next = s[start + open.len()..].chars().next();
        match next {
            Some(c) if c == '>' || c == '/' || c.is_whitespace() => return Some(start),
            _ => pos = start + open.len(),
        }
    }
    None
}

/// The content between `<name ...>` and `</name>` of the first such
/// element; a self-closing element gives `""`.
fn element_body<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    next_element(s, name).map(|(_, body, _)| body)
}

/// The first element `name` in `s`: its attribute text, its content and
/// the rest of `s` after it. `None` if there is none or it is not closed.
fn next_element<'a>(s: &'a str, name: &str) -> Option<(&'a str, &'a str, &'a str)> {
    let start = find_start_tag(s, name, 0)?;
    let tag_from = start + 1 + name.len();
    let tag_end = tag_from + s[tag_from..].find('>')?;
    let attrs = &s[tag_from..tag_end];
    if let Some(attrs) = attrs.strip_suffix('/') {
        return Some((attrs, "", &s[tag_end + 1..]));
    }
    let close = format!("</{name}>");
    let body_from = tag_end + 1;
    let body_end = body_from + s[body_from..].find(&close)?;
    Some((attrs, &s[body_from..body_end], &s[body_end + close.len()..]))
}

/// Whether the attribute text holds `xml:lang="x-default"` (either quote).
fn is_x_default(attrs: &str) -> bool {
    attrs
        .split_whitespace()
        .any(|a| a == "xml:lang=\"x-default\"" || a == "xml:lang='x-default'")
}

/// Decodes the five named XML entities and numeric character references.
/// Anything that is not a valid entity is kept as it is.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail
            .find(';')
            .and_then(|j| entity(&tail[1..j]).map(|c| (c, j)))
        {
            Some((c, j)) => {
                out.push(c);
                rest = &tail[j + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The character of one entity name (without `&` and `;`).
fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let num = name.strip_prefix('#')?;
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    fn blob(xmp: &[u8]) -> Vec<u8> {
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(xmp).unwrap();
        let mut b = vec![0, 0, 0, 0];
        b.extend(enc.finish().unwrap());
        b
    }

    fn alt(items: &str) -> String {
        format!(
            "<rdf:Description><dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">d</rdf:li>\
             </rdf:Alt></dc:description><dc:title><rdf:Alt>{items}</rdf:Alt></dc:title>\
             </rdf:Description>"
        )
    }

    #[test]
    fn x_default_wins_else_first_entry() {
        let x = alt("<rdf:li xml:lang=\"de\">a</rdf:li><rdf:li xml:lang=\"x-default\">b</rdf:li>");
        assert_eq!(title_from_xmp(&x).as_deref(), Some("b"));
        let x = alt("<rdf:li>a</rdf:li><rdf:li xml:lang=\"fr\">b</rdf:li>");
        assert_eq!(title_from_xmp(&x).as_deref(), Some("a"));
        let x = alt("<rdf:li xml:lang='x-default'>c</rdf:li>");
        assert_eq!(title_from_xmp(&x).as_deref(), Some("c"));
    }

    #[test]
    fn description_is_not_the_title() {
        let x = "<dc:description><rdf:Alt><rdf:li>d</rdf:li></rdf:Alt></dc:description>";
        assert_eq!(title_from_xmp(x), None);
        let x = "<dc:titles><rdf:Alt><rdf:li>d</rdf:li></rdf:Alt></dc:titles>";
        assert_eq!(title_from_xmp(x), None);
    }

    #[test]
    fn empty_blank_and_self_closing_give_none() {
        assert_eq!(
            title_from_xmp(&alt("<rdf:li xml:lang=\"x-default\"></rdf:li>")),
            None
        );
        assert_eq!(
            title_from_xmp(&alt("<rdf:li xml:lang=\"x-default\"/>")),
            None
        );
        assert_eq!(title_from_xmp(&alt("<rdf:li> \n </rdf:li>")), None);
        assert_eq!(title_from_xmp("<dc:title/>"), None);
        assert_eq!(title_from_xmp(&alt("")), None);
    }

    #[test]
    fn text_is_kept_unchanged_and_entities_decoded() {
        let x = alt(
            "<rdf:li> a &amp;b &lt;&gt;&quot;&apos;&#65;&#x42;&#x1F305; &bad; &#xZZ; & </rdf:li>",
        );
        assert_eq!(
            title_from_xmp(&x).as_deref(),
            Some(" a &b <>\"'AB\u{1f305} &bad; &#xZZ; & ")
        );
    }

    #[test]
    fn blobs_unpack_or_are_unreadable() {
        let x = alt("<rdf:li>t</rdf:li>");
        assert_eq!(title_from_blob(&blob(x.as_bytes())), Ok(Some("t".into())));
        assert_eq!(title_from_blob(&blob(b"<x/>")), Ok(None));
        assert_eq!(title_from_blob(&[0, 0, 1]), Err(Unreadable));
        assert_eq!(title_from_blob(&[0, 0, 0, 1, 2, 3, 4]), Err(Unreadable));
        assert_eq!(title_from_blob(&blob(&[0xff, 0xfe])), Err(Unreadable));
        assert_eq!(title_from_blob(&[]), Err(Unreadable));
    }

    #[test]
    fn oversized_packets_are_unreadable() {
        let big = vec![b' '; MAX_XMP_BYTES as usize + 1];
        assert_eq!(unpack(&blob(&big)), Err(Unreadable));
        let ok = vec![b' '; 1024];
        assert!(unpack(&blob(&ok)).is_ok());
    }
}
