//! Forgiving parser for the XML-like `.wms` format: case-insensitive tags, unquoted attribute
//! values, stray `&`, and mismatched or missing close tags are all tolerated as WMP did.

/// One element of a skin definition. The tag is upper-case; attribute names keep their spelling.
#[derive(Debug, Default, Clone)]
pub struct WmsNode {
    pub tag: String,
    pub line: usize,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<WmsNode>,
}

impl WmsNode {
    /// The last value given for an attribute (a repeated attribute keeps its last value).
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.iter().rev().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn descendants(&self) -> Vec<&WmsNode> {
        self.children.iter().flat_map(|c| std::iter::once(c).chain(c.descendants())).collect()
    }
}

pub fn parse(text: &str) -> WmsNode {
    let t: Vec<char> = text.chars().collect();
    let n = t.len();
    // The stack holds nodes being filled; a finished node is moved into its parent.
    let mut stack = vec![WmsNode { tag: "#ROOT".into(), ..Default::default() }];
    let (mut i, mut line) = (0usize, 1usize);

    let find = |from: usize, pat: &str| -> Option<usize> {
        let p: Vec<char> = pat.chars().collect();
        (from..n.saturating_sub(p.len() - 1)).find(|&k| t[k..k + p.len()] == p[..])
    };
    let starts = |at: usize, pat: &str| pat.chars().enumerate().all(|(k, c)| t.get(at + k) == Some(&c));
    let count_lines = |from: usize, to: usize| t[from.min(n)..to.min(n)].iter().filter(|&&c| c == '\n').count();

    fn close(stack: &mut Vec<WmsNode>) {
        let node = stack.pop().expect("never pops the root");
        stack.last_mut().expect("root stays").children.push(node);
    }

    while i < n {
        let Some(lt) = (i..n).find(|&k| t[k] == '<') else { break };
        line += count_lines(i, lt);
        i = lt;

        if starts(i, "<!--") {
            let end = find(i + 4, "-->").map_or(n, |e| e + 3);
            line += count_lines(i, end);
            i = end;
            continue;
        }
        if starts(i, "<![CDATA[") {
            i = find(i, "]]>").map_or(n, |e| e + 3);
            continue;
        }
        if starts(i, "<?") || starts(i, "<!") {
            i = (i..n).find(|&k| t[k] == '>').map_or(n, |e| e + 1);
            continue;
        }
        if starts(i, "</") {
            let end = (i..n).find(|&k| t[k] == '>').unwrap_or(n);
            let name: String = t[i + 2..end].iter().collect::<String>().trim().to_uppercase();
            i = (end + 1).min(n);
            // Pop to the matching open tag if there is one; otherwise ignore the stray close.
            if stack.iter().skip(1).any(|s| s.tag == name) {
                while stack.len() > 1 {
                    let is_match = stack.last().is_some_and(|s| s.tag == name);
                    close(&mut stack);
                    if is_match {
                        break;
                    }
                }
            }
            continue;
        }

        // Start tag.
        let mut p = i + 1;
        let name_start = p;
        while p < n && (t[p].is_alphanumeric() || matches!(t[p], '_' | ':' | '-' | '.')) {
            p += 1;
        }
        if p == name_start {
            i += 1;
            continue;
        }
        let mut node = WmsNode { tag: t[name_start..p].iter().collect::<String>().to_uppercase(), line, ..Default::default() };
        let mut self_close = false;

        while p < n {
            while p < n && t[p].is_whitespace() {
                line += usize::from(t[p] == '\n');
                p += 1;
            }
            if p >= n {
                break;
            }
            if t[p] == '>' {
                p += 1;
                break;
            }
            if t[p] == '/' && t.get(p + 1) == Some(&'>') {
                self_close = true;
                p += 2;
                break;
            }
            if t[p] == '/' {
                p += 1;
                continue;
            }
            let attr_start = p;
            while p < n && !t[p].is_whitespace() && t[p] != '=' && t[p] != '>' && !(t[p] == '/' && t.get(p + 1) == Some(&'>')) {
                p += 1;
            }
            let attr_name: String = t[attr_start..p].iter().collect();
            while p < n && t[p].is_whitespace() {
                line += usize::from(t[p] == '\n');
                p += 1;
            }
            let mut value = String::new();
            if p < n && t[p] == '=' {
                p += 1;
                while p < n && t[p].is_whitespace() {
                    line += usize::from(t[p] == '\n');
                    p += 1;
                }
                if p < n && (t[p] == '"' || t[p] == '\'') {
                    let q = t[p];
                    p += 1;
                    let vs = p;
                    let ve = (p..n).find(|&k| t[k] == q).unwrap_or(n);
                    value = t[vs..ve].iter().collect();
                    line += count_lines(vs, ve);
                    p = (ve + 1).min(n);
                } else {
                    let vs = p;
                    while p < n && !t[p].is_whitespace() && t[p] != '>' {
                        p += 1;
                    }
                    if p > vs && t[p - 1] == '/' && p < n && t[p] == '>' {
                        p -= 1;
                    }
                    value = t[vs..p].iter().collect();
                }
            }
            if !attr_name.is_empty() {
                node.attributes.push((attr_name, decode_entities(&value)));
            }
        }
        i = p;
        stack.push(node);
        if self_close {
            close(&mut stack);
        }
    }
    while stack.len() > 1 {
        close(&mut stack);
    }
    stack.pop().expect("root")
}

/// HTML entity decoding for attribute values (`&amp;`, `&lt;`, `&#39;`, `&#x27;`, ...).
fn decode_entities(v: &str) -> String {
    if !v.contains('&') {
        return v.to_string();
    }
    let mut out = String::with_capacity(v.len());
    let mut rest = v;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let decoded = tail.find(';').filter(|&semi| semi <= 10).and_then(|semi| {
            let ent = &tail[1..semi];
            let c = match ent {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
                _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
                _ => None,
            };
            c.map(|c| (c, semi + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &tail[len..];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_loose_markup() {
        let root = parse(
            "<?xml version=\"1.0\"?>\n<!-- c -->\n<theme title='T'>\n<view id=main clippingColor=#FF00FF>\n<button id=\"b\" onclick=\"a &amp;&amp; b; x & y\"/>\n<SubView id=s><text value=hi></subview>\n</VIEW></Theme>",
        );
        let theme = &root.children[0];
        assert_eq!(theme.tag, "THEME");
        let view = &theme.children[0];
        assert_eq!(view.attr("ID"), Some("main"));
        assert_eq!(view.attr("clippingcolor"), Some("#FF00FF"));
        assert_eq!(view.line, 4);
        assert_eq!(view.children[0].attr("onclick"), Some("a && b; x & y"));
        // The unclosed <text> closes with its subview.
        assert_eq!(view.children[1].children[0].tag, "TEXT");
        assert_eq!(view.children.len(), 2);
    }

    #[test]
    fn ignores_stray_close_tags_and_keeps_last_repeated_attribute() {
        let root = parse("<VIEW a=1 a=2></BUTTON><TEXT/></VIEW>");
        assert_eq!(root.children[0].attr("a"), Some("2"));
        assert_eq!(root.children[0].children[0].tag, "TEXT");
    }
}
