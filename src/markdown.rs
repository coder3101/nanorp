//! Markdown-to-HTML rendering for chat messages, shared by SSR and the WASM
//! client. Raw HTML in the (untrusted) model output is escaped, so the result
//! is safe to inject via `inner_html`.

use pulldown_cmark::{html, Event, Options, Parser, Tag};

/// Model output is untrusted, including link destinations. Keep ordinary web,
/// mail, relative, and fragment links while dropping executable/unknown schemes.
fn safe_destination(url: &str) -> bool {
    let url = url.trim();
    let lower = url.to_lowercase();
    lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || url.starts_with('/')
        || url.starts_with('#')
        || url.starts_with("./")
        || url.starts_with("../")
        || !url.contains(':')
}

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
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) if !safe_destination(&dest_url) => Event::Start(Tag::Link {
            link_type,
            dest_url: "".into(),
            title,
            id,
        }),
        // Remote Markdown images are not needed for roleplay replies; block
        // unsafe destinations as well as turning unsafe URLs into broken images.
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) if !safe_destination(&dest_url) => Event::Start(Tag::Image {
            link_type,
            dest_url: "".into(),
            title,
            id,
        }),
        Event::Html(s) | Event::InlineHtml(s) => Event::Text(s),
        Event::SoftBreak => Event::HardBreak,
        other => other,
    });

    let mut out = String::new();
    html::push_html(&mut out, parser);
    // Raw HTML is escaped above, so every `<a ` here was emitted by the renderer
    // for a destination that already passed `safe_destination`. Opening links in
    // a new tab keeps the conversation from being navigated away, and `rel`
    // prevents the opened page from gaining access to this one.
    out.replace(
        "<a href=",
        "<a target=\"_blank\" rel=\"noopener noreferrer\" href=",
    )
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

    #[test]
    fn blocks_executable_link_and_image_destinations() {
        let html = render_markdown("[click](javascript:alert(1)) ![x](data:text/html,test)");
        assert!(!html.contains("href=\"javascript:"));
        assert!(!html.contains("src=\"data:"));
    }

    #[test]
    fn permits_normal_link_destinations() {
        let html = render_markdown("[web](https://example.com) [relative](#section)");
        assert!(html.contains("https://example.com"));
        assert!(html.contains("#section"));
    }

    #[test]
    fn links_open_in_a_new_tab_safely() {
        let html = render_markdown("[web](https://example.com)");
        assert!(html.contains("target=\"_blank\""));
        assert!(html.contains("rel=\"noopener noreferrer\""));
    }
}
