//! Markdown-to-HTML rendering for chat messages, shared by SSR and the WASM
//! client. Raw HTML in the (untrusted) model output is escaped, so the result
//! is safe to inject via `inner_html`.

use pulldown_cmark::{html, Event, Options, Parser};

/// Render markdown to sanitized HTML.
///
/// - Tables and strikethrough are enabled (common in LLM output).
/// - Soft breaks (single newlines) become hard `<br>` breaks, preserving the
///   line-by-line feel of chat and roleplay replies.
/// - Raw HTML is escaped rather than passed through.
pub fn render_markdown(text: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(text, options).map(|event| match event {
        Event::Html(s) | Event::InlineHtml(s) => Event::Text(s),
        Event::SoftBreak => Event::HardBreak,
        other => other,
    });

    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

#[cfg(test)]
mod tests {
    use super::render_markdown;

    #[test]
    fn renders_basic_markdown() {
        let html = render_markdown("**bold** and *italic* and `code`");
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
        assert!(html.contains("<code>code</code>"));
    }

    #[test]
    fn renders_lists_and_headings() {
        let html = render_markdown("# Title\n\n- one\n- two");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<li>one</li>"));
    }

    #[test]
    fn escapes_raw_html() {
        let html = render_markdown("hello <script>alert(1)</script> world");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn escapes_html_blocks() {
        let html = render_markdown("<img src=x onerror=alert(1)>");
        assert!(!html.contains("<img"));
    }

    #[test]
    fn single_newlines_become_line_breaks() {
        let html = render_markdown("line one\nline two");
        assert!(html.contains("<br"));
    }

    #[test]
    fn renders_code_blocks() {
        let html = render_markdown("```rust\nfn main() {}\n```");
        assert!(html.contains("<pre>"));
        assert!(html.contains("fn main() {}"));
    }
}
