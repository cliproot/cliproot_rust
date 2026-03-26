/// Build the HTML clipboard payload mirroring `writeProvenanceToClipboard`
/// from `packages/core/src/clipboard-writer.ts`:
///
///   <span>{escaped text}</span>
///   <div style="display:none" data-crp-bundle="{escaped json}"></div>
pub fn build_provenance_html(plain_text: &str, bundle_json: &str) -> String {
    let escaped_text = escape_html_content(plain_text);
    let escaped_bundle = escape_attr(bundle_json);
    format!(
        r#"<span>{escaped_text}</span><div style="display:none" data-crp-bundle="{escaped_bundle}"></div>"#
    )
}

fn escape_html_content(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_html_entities_in_text() {
        let html = build_provenance_html("a < b & c > d", r#"{"k":"v"}"#);
        assert!(html.contains("<span>a &lt; b &amp; c &gt; d</span>"));
    }

    #[test]
    fn escapes_quotes_in_attribute() {
        let html = build_provenance_html("text", r#"{"key":"val\"ue"}"#);
        assert!(html.contains("data-crp-bundle="));
        assert!(!html.contains(r#"val"ue"#));
    }

    #[test]
    fn structure_matches_js_implementation() {
        let html = build_provenance_html("hello", r#"{"foo":"bar"}"#);
        assert!(html.starts_with("<span>hello</span>"));
        assert!(html.contains(r#"<div style="display:none" data-crp-bundle=""#));
    }
}
