use pulldown_cmark::{Options, Parser, html};

const CSS_COMMON: &str = r#"
body{font:15px/1.6 -apple-system,"Cantarell","Segoe UI",system-ui,sans-serif;margin:0;padding:28px 36px;max-width:820px;margin-inline:auto}
h1,h2,h3,h4{line-height:1.25;margin:1.4em 0 .5em}h1{font-size:1.7em}h2{font-size:1.35em}h3{font-size:1.15em}
a{text-decoration:none}a:hover{text-decoration:underline}
img{max-width:100%}
pre{padding:12px 14px;border-radius:8px;overflow:auto}
code{font-family:"Source Code Pro",ui-monospace,Menlo,monospace;font-size:.92em}
:not(pre)>code{padding:.1em .35em;border-radius:4px}
blockquote{margin:1em 0;padding:.1em 1em;border-left:3px solid}
table{border-collapse:collapse}th,td{padding:6px 10px;border:1px solid}
ul:has(>li>input[type=checkbox]){list-style:none;padding-left:.4em}
input[type=checkbox]{margin-right:.5em}
hr{border:0;border-top:1px solid}
"#;
const CSS_LIGHT: &str = ":root{color-scheme:light}body{background:#fff;color:#2c3338}a{color:#3361d8}pre,:not(pre)>code{background:#f3f4f6}blockquote{border-color:#c9ced6;color:#5b6470}th,td,hr{border-color:#d7dbe0}";
const CSS_DARK: &str = ":root{color-scheme:dark}body{background:#1e1e1e;color:#e4e6e8}a{color:#7aa2ff}pre,:not(pre)>code{background:#2b2d30}blockquote{border-color:#4a4f57;color:#aab0b8}th,td,hr{border-color:#3d4148}";

/// Render Markdown to a sanitized, script-free, self-contained HTML document
/// for the read-only preview (WebKitGTK / WKWebView).
pub fn render_html(markdown: &str, dark: bool) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src https: http: data:; style-src 'unsafe-inline'\">\
<style>{CSS_COMMON}{}</style></head><body>{}</body></html>",
        if dark { CSS_DARK } else { CSS_LIGHT },
        render_body(markdown)
    )
}

pub fn render_body(markdown: &str) -> String {
    let opts =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES;
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(markdown, opts));
    let mut b = ammonia::Builder::default();
    b.add_tags(["input"])
        .add_tag_attributes("input", ["type", "checked", "disabled"])
        .link_rel(Some("noopener noreferrer"));
    b.clean(&out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_tasks_and_strips_scripts() {
        let h = render_body("# Hi\n\n- [x] done\n- [ ] todo\n\n<script>alert(1)</script>");
        assert!(h.contains("<h1>Hi</h1>"));
        assert!(h.contains("type=\"checkbox\""));
        assert!(h.contains("checked"));
        assert!(!h.contains("script"));
    }

    #[test]
    fn dark_and_light_documents() {
        assert!(render_html("x", true).contains("color-scheme:dark"));
        assert!(render_html("x", false).contains("color-scheme:light"));
    }
}
