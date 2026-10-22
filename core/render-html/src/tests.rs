//! Role: Verify HTML rendering, sanitization, and navigation behavior.

use super::*;
use fleximark_model::DocumentUri;
use fleximark_parser::parse;

#[test]
fn renders_typed_special_blocks_with_owned_identity() {
    let source = "# Hi\n\n| A |\n|---|\n| B |\n\n```mermaid\ngraph TD; A-->B\n```\n";
    let document = parse(DocumentUri("file:///demo.md".into()), 1, source).unwrap();
    let html = HtmlRenderer
        .render(&document, &RenderContext::default())
        .unwrap();
    assert!(html.contains("data-fleximark-node-id=\"pending-0\""));
    assert!(html.contains("data-fleximark-kind=\"mermaid\""));
    assert!(html.contains("<th data-fleximark-node-id="));
    assert!(html.contains(r#"type="application/json">"graph TD; A--\u003eB\n""#));
}

#[test]
fn marks_only_tight_lists_for_compact_preview_spacing() {
    let tight = parse(DocumentUri("file:///tight.md".into()), 1, "- one\n- two\n").unwrap();
    let loose = parse(
        DocumentUri("file:///loose.md".into()),
        1,
        "- one\n\n- two\n",
    )
    .unwrap();

    let tight_html = HtmlRenderer
        .render(&tight, &RenderContext::default())
        .unwrap();
    let loose_html = HtmlRenderer
        .render(&loose, &RenderContext::default())
        .unwrap();
    assert!(tight_html.contains("data-fleximark-tight=\"true\""));
    assert!(!loose_html.contains("data-fleximark-tight"));
}

#[test]
fn renders_accessible_tabs_details_and_visible_code_metadata() {
    let source = "::::tabs\n:::tab[First]\none\n:::\n:::tab[Second]\ntwo\n:::\n::::\n\n:::details[Read more]\ninside\n:::\n\n```rust title='Demo' line-numbers\nfn main() {}\nlet x = 2;\n```\n";
    let document = parse(DocumentUri("file:///ui.md".into()), 1, source).unwrap();
    let html = HtmlRenderer
        .render(&document, &RenderContext::default())
        .unwrap();
    assert!(html.contains("data-fleximark-kind=\"tab\" data-tab-label=\"First\""));
    assert!(html.contains("<summary>Read more</summary>"));
    assert!(html.contains("<figcaption class=\"fleximark-code-title\">Demo</figcaption>"));
    assert!(html.contains("class=\"fleximark-code-line\" data-line=\"1\""));
    assert!(html.contains("class=\"fleximark-code-line\" data-line=\"2\""));
}

#[test]
fn sanitizes_raw_html_links_and_image_resources() {
    let raw = parse(
        DocumentUri("file:///raw.md".into()),
        1,
        "<script>alert(1)</script>\n",
    )
    .unwrap();
    let sanitized = HtmlRenderer
        .render(&raw, &RenderContext::default())
        .unwrap();
    assert!(!sanitized.contains("script"));
    assert!(!sanitized.contains("alert(1)"));
    let link = parse(
        DocumentUri("file:///link.md".into()),
        1,
        "[bad](javascript:alert(1))\n",
    )
    .unwrap();
    assert!(matches!(
        HtmlRenderer.render(&link, &RenderContext::default()),
        Err(RenderError::ResourceRejected(_))
    ));
    let permissive = RenderContext {
        allow_remote_resources: true,
        allow_data_resources: true,
        ..RenderContext::default()
    };
    assert!(resolve_resource("//evil.example/x", &permissive, ResourceUse::Image).is_err());
    assert!(resolve_resource("data:text/html,<script>", &permissive, ResourceUse::Image).is_err());
    assert!(resolve_resource("data:image/svg+xml,<svg>", &permissive, ResourceUse::Image).is_err());
    assert!(
        resolve_resource(
            "data:image/png,percent-encoded",
            &permissive,
            ResourceUse::Image
        )
        .is_err()
    );
    assert!(
        resolve_resource(
            "data:image/avif;base64,AA==",
            &permissive,
            ResourceUse::Image
        )
        .is_err()
    );
    assert!(
        resolve_resource(
            "data:image/png;base64,AA==",
            &permissive,
            ResourceUse::Image
        )
        .is_ok()
    );
    assert!(
        resolve_resource("data:image/png;base64,AA==", &permissive, ResourceUse::Link).is_err()
    );
}

#[test]
fn preserves_useful_raw_html_and_inline_tag_pairs() {
    let source = "Press <kbd class=\"key\">Ctrl</kbd> and <mark>Enter</mark>.\n\n<details open><summary>More</summary><table><tr><th scope=\"col\">A</th></tr><tr><td>1</td></tr></table></details>\n";
    let document = parse(DocumentUri("file:///safe-html.md".into()), 1, source).unwrap();
    let html = HtmlRenderer
        .render(&document, &RenderContext::default())
        .unwrap();

    assert!(html.contains("<kbd class=\"key\">Ctrl</kbd>"));
    assert!(html.contains("<mark>Enter</mark>"));
    assert!(html.contains("<details open"));
    assert!(html.contains("<summary>More</summary>"));
    assert!(html.contains("<th scope=\"col\">A</th>"));
}

#[test]
fn raw_details_container_keeps_markdown_blocks_inside_across_block_boundaries() {
    let source = "<details><summary>More</summary>\n\nBody **bold**\n\n</details>\n";
    let document = parse(DocumentUri("file:///details.md".into()), 1, source).unwrap();
    assert!(
        document.blocks.len() >= 3,
        "fixture must cross block boundaries"
    );

    let blocks = HtmlRenderer
        .render_blocks(&document, &RenderContext::default())
        .unwrap();
    assert_eq!(
        blocks.len(),
        1,
        "the cross-block container is one safe patch unit"
    );
    let html = &blocks[0].html;
    let details = html.find("<details>").unwrap();
    let body = html.find(">Body <strong>bold</strong></p>").unwrap();
    let closing = html.rfind("</details>").unwrap();
    assert!(details < body && body < closing, "{html}");
    assert_eq!(blocks[0].node_ids.len(), document.blocks.len());
}

#[test]
fn nested_raw_containers_and_nested_block_sequences_preserve_structure() {
    let source = concat!(
        "<details><summary>Outer</summary>\n\n",
        "<details><summary>Inner</summary>\n\n",
        "Nested body\n\n",
        "</details>\n\n",
        "Outer tail\n\n",
        "</details>\n"
    );
    let document = parse(DocumentUri("file:///nested-details.md".into()), 1, source).unwrap();
    let html = HtmlRenderer
        .render(&document, &RenderContext::default())
        .unwrap();
    assert_eq!(html.matches("<details>").count(), 2, "{html}");
    let nested_body = html.find(">Nested body</p>").unwrap();
    let inner_close = html.find("</details>").unwrap();
    let outer_tail = html.find(">Outer tail</p>").unwrap();
    let outer_close = html.rfind("</details>").unwrap();
    assert!(nested_body < inner_close && inner_close < outer_tail && outer_tail < outer_close);

    let quoted = parse(
        DocumentUri("file:///quoted-details.md".into()),
        1,
        "> <details><summary>Nested</summary>\n>\n> body\n>\n> </details>\n",
    )
    .unwrap();
    let quoted_html = HtmlRenderer
        .render(&quoted, &RenderContext::default())
        .unwrap();
    let open = quoted_html.find("<details>").unwrap();
    let body = quoted_html.find(">body</p>").unwrap();
    let close = quoted_html.find("</details>").unwrap();
    assert!(open < body && body < close, "{quoted_html}");
}

#[test]
fn table_and_list_containers_survive_raw_block_boundaries() {
    let source = concat!(
        "<table>\n\n",
        "<tr><th>A</th></tr>\n\n",
        "<tr><td>1</td></tr>\n\n",
        "</table>\n\n",
        "<ul>\n\n",
        "<li>one</li>\n\n",
        "<li>two</li>\n\n",
        "</ul>\n"
    );
    let document = parse(DocumentUri("file:///table-list.md".into()), 1, source).unwrap();
    assert!(
        document.blocks.len() >= 8,
        "fixture must cross raw block boundaries"
    );
    let blocks = HtmlRenderer
        .render_blocks(&document, &RenderContext::default())
        .unwrap();
    let html = blocks
        .iter()
        .map(|block| block.html.as_str())
        .collect::<String>();
    let table_open = html.find("<table>").unwrap();
    let table_row = html.find("<tr><td>1</td></tr>").unwrap();
    let table_close = html.find("</table>").unwrap();
    let list_open = html.find("<ul>").unwrap();
    let list_item = html.find("<li>two</li>").unwrap();
    let list_close = html.find("</ul>").unwrap();
    assert!(table_open < table_row && table_row < table_close, "{html}");
    assert!(list_open < list_item && list_item < list_close, "{html}");

    let node_ids = blocks
        .iter()
        .flat_map(|block| block.node_ids.iter())
        .collect::<HashSet<_>>();
    assert_eq!(
        node_ids.len(),
        blocks
            .iter()
            .map(|block| block.node_ids.len())
            .sum::<usize>()
    );
}

#[test]
fn malformed_closing_tag_cannot_capture_the_following_block_after_browser_style_reparse() {
    let document = parse(
        DocumentUri("file:///malformed-close.md".into()),
        1,
        "</details>\n\nFollowing paragraph\n",
    )
    .unwrap();
    let blocks = HtmlRenderer
        .render_blocks(&document, &RenderContext::default())
        .unwrap();
    assert_eq!(blocks.len(), 2);
    assert!(
        blocks[0]
            .html
            .contains("data-fleximark-kind=\"raw-html-container\"")
    );
    assert!(blocks[1].html.contains(">Following paragraph</p>"));

    // Ammonia and browsers both use the HTML5 tree-builder rules. Parsing
    // the emitted blocks again must retain two sibling roots rather than
    // letting an unmatched close tag capture the following paragraph.
    let reparsed = clean_raw_html(
        &blocks
            .iter()
            .map(|block| block.html.as_str())
            .collect::<String>(),
    );
    assert!(reparsed.starts_with("<div>"), "{reparsed}");
    assert!(
        reparsed.contains("</div>\n<p>Following paragraph</p>"),
        "{reparsed}"
    );

    let node_ids = blocks
        .iter()
        .flat_map(|block| block.node_ids.iter())
        .collect::<HashSet<_>>();
    assert_eq!(node_ids.len(), 2);
}

#[test]
fn author_block_html_cannot_collide_with_internal_sequence_markers() {
    let source = concat!(
        "<details><summary>Markers</summary>\n\n",
        "<FLEXIMARK-BLOCK-MARKER data-index=\"0\"></FLEXIMARK-BLOCK-MARKER>\n",
        "<fleximark-safe-block data-index=\"1\"></fleximark-safe-block>\n\n",
        "trusted body\n\n",
        "</details>\n"
    );
    let document = parse(DocumentUri("file:///block-marker.md".into()), 1, source).unwrap();
    let html = HtmlRenderer
        .render(&document, &RenderContext::default())
        .unwrap();
    assert!(html.contains("&lt;FLEXIMARK-BLOCK-MARKER"), "{html}");
    assert!(html.contains("&lt;fleximark-safe-block"), "{html}");
    assert_eq!(html.matches(">trusted body</p>").count(), 1, "{html}");
}

#[test]
fn author_html_cannot_collide_with_internal_inline_fragment_markers() {
    let source = "Attempt <FLEXIMARK-SAFE-FRAGMENT data-index=\"0\"></FLEXIMARK-SAFE-FRAGMENT> then <kbd>Ctrl</kbd> and **trusted**.\n";
    let document = parse(DocumentUri("file:///marker.md".into()), 1, source).unwrap();
    let html = HtmlRenderer
        .render(&document, &RenderContext::default())
        .unwrap();

    assert!(html.contains("&lt;FLEXIMARK-SAFE-FRAGMENT"));
    assert_eq!(html.matches("<strong>trusted</strong>").count(), 1);
    assert!(html.contains("<kbd>Ctrl</kbd>"));
}

#[test]
fn raw_html_sanitizer_keeps_author_markup_but_removes_executable_and_embedded_content() {
    let source = concat!(
        "<div id=\"preview\" class=\"box\" style=\"background:url(https://evil.example)\" data-note=\"kept\" aria-label=\"example\" onclick=\"steal()\">",
        "<script>steal()</script><style>@import 'https://evil.example/x';</style>",
        "<iframe src=\"https://evil.example\">fallback</iframe>",
        "<svg onload=\"steal()\"><script>steal()</script></svg>",
        "<img src=\"https://evil.example/pixel\">",
        "<a href=\"javascript:steal()\" data-fleximark-node-id=\"forged\">bad</a>",
        "<a href=\"//evil.example/path\">protocol-relative</a>",
        "<a href=\"../guide\">relative</a>",
        "<a href=\"https://example.com\">good</a></div>"
    );
    let html = clean_raw_html(source);

    assert!(html.contains("id=\"preview\""));
    assert!(html.contains("class=\"box\""));
    assert!(html.contains("style=\"background:url(https://evil.example)\""));
    assert!(html.contains("data-note=\"kept\""));
    assert!(html.contains("aria-label=\"example\""));
    assert!(!html.contains("onclick"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("steal"));
    assert!(!html.contains("<iframe"));
    assert!(!html.contains("<svg"));
    assert!(html.contains("<img src=\"https://evil.example/pixel\">"));
    assert!(!html.contains("data-fleximark-node-id"));
    assert!(html.contains("<a>bad</a>"));
    assert!(html.contains("<a>protocol-relative</a>"));
    assert!(html.contains("<a href=\"../guide\">relative</a>"));
    assert!(html.contains("<a href=\"https://example.com\">good</a>"));
}

#[test]
fn local_images_require_a_resolved_opaque_asset_reference() {
    let document = parse(
        DocumentUri("file:///asset.md".into()),
        1,
        "![diagram](images/diagram.png)\n",
    )
    .unwrap();
    let unresolved = HtmlRenderer
        .render(&document, &RenderContext::default())
        .unwrap();
    assert!(unresolved.contains("data-fleximark-kind=\"asset-placeholder\""));
    assert!(!unresolved.contains("images/diagram.png"));

    let mut context = RenderContext::default();
    context.resolved_resources.insert(
        "images/diagram.png".to_owned(),
        format!("fleximark-asset:{}", "a".repeat(64)),
    );
    let html = HtmlRenderer.render(&document, &context).unwrap();
    assert!(html.contains(&format!("src=\"fleximark-asset:{}\"", "a".repeat(64))));
    assert!(!html.contains("images/diagram.png"));

    let portable = RenderContext {
        target: HtmlTarget::Portable,
        ..RenderContext::default()
    };
    let html = HtmlRenderer.render(&document, &portable).unwrap();
    assert!(html.contains("src=\"images/diagram.png\""));
    assert!(matches!(
        resolve_resource(
            "https://outside.example/diagram.png",
            &portable,
            ResourceUse::Image
        ),
        Err(RenderError::ResourceRejected(_))
    ));

    let document = parse(
        DocumentUri("file:///assets.md".into()),
        1,
        "![works](images/diagram.png)\n\n![blocked](../outside.png)\n",
    )
    .unwrap();
    let html = HtmlRenderer.render(&document, &context).unwrap();
    assert!(html.contains(&format!("src=\"fleximark-asset:{}\"", "a".repeat(64))));
    assert!(html.contains("aria-label=\"blocked\""));
    assert!(!html.contains("../outside.png"));
}

#[test]
fn rendered_metadata_contains_valid_navigation_for_nested_unicode_blocks() {
    let source = "- Héllo\n";
    let document = parse(DocumentUri("file:///nested.md".into()), 1, source).unwrap();
    document.validate(source).unwrap();
    let blocks = HtmlRenderer
        .render_blocks(&document, &RenderContext::default())
        .unwrap();
    let navigation = blocks
        .iter()
        .flat_map(|block| &block.navigation)
        .collect::<Vec<_>>();
    assert_eq!(navigation.len(), blocks[0].node_ids.len());
    assert!(navigation.iter().any(|entry| entry.depth >= 2));
    assert!(navigation.iter().all(|entry| {
        entry.source_range.byte_end <= source.len() as u64
            && source.is_char_boundary(entry.source_range.byte_start as usize)
            && source.is_char_boundary(entry.source_range.byte_end as usize)
    }));
}
