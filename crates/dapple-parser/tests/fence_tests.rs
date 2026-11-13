use dapple_parser::fence::{FenceToken, tokenize};

#[test]
fn test_tokenize_simple_block() {
    let content = r#"Some prose.

:::block { tags: [agent] }
Tagged content here.
:::

More prose."#;

    let tokens = tokenize(content).unwrap();

    // Count token types
    let prose_count = tokens
        .iter()
        .filter(|t| matches!(t, FenceToken::Prose { .. }))
        .count();
    let open_count = tokens
        .iter()
        .filter(|t| matches!(t, FenceToken::Open { .. }))
        .count();
    let close_count = tokens
        .iter()
        .filter(|t| matches!(t, FenceToken::Close { .. }))
        .count();
    let content_count = tokens
        .iter()
        .filter(|t| matches!(t, FenceToken::Content { .. }))
        .count();

    assert_eq!(open_count, 1, "should have one Open token");
    assert_eq!(close_count, 1, "should have one Close token");
    assert_eq!(content_count, 1, "should have one Content token");
    assert!(prose_count >= 2, "should have at least two Prose tokens");

    // Verify the Open token has the right directive
    let open_token = tokens
        .iter()
        .find(|t| matches!(t, FenceToken::Open { .. }))
        .unwrap();
    if let FenceToken::Open {
        directive,
        raw_attrs,
        ..
    } = open_token
    {
        assert_eq!(directive, "block");
        assert!(raw_attrs.contains("tags"));
    }
}

#[test]
fn test_tokenize_nested_blocks() {
    let content = r#":::stage { name: "Setup" }
:::step { name: "Info" }
Content
:::
:::
"#;
    let tokens = tokenize(content).unwrap();

    let open_count = tokens
        .iter()
        .filter(|t| matches!(t, FenceToken::Open { .. }))
        .count();
    let close_count = tokens
        .iter()
        .filter(|t| matches!(t, FenceToken::Close { .. }))
        .count();

    assert_eq!(open_count, 2, "should have two Open tokens");
    assert_eq!(close_count, 2, "should have two Close tokens");

    // Verify the directives are correct
    let opens: Vec<&FenceToken> = tokens
        .iter()
        .filter(|t| matches!(t, FenceToken::Open { .. }))
        .collect();
    if let FenceToken::Open { directive, .. } = opens[0] {
        assert_eq!(directive, "stage");
    }
    if let FenceToken::Open { directive, .. } = opens[1] {
        assert_eq!(directive, "step");
    }
}

#[test]
fn test_code_blocks_not_matched() {
    let content = r#"```
:::stage { name: "not a stage" }
```"#;
    let tokens = tokenize(content).unwrap();
    // All should be Prose -- code block contents are opaque
    for token in &tokens {
        assert!(
            matches!(token, FenceToken::Prose { .. }),
            "expected Prose, got {:?}",
            token
        );
    }
}

#[test]
fn test_code_blocks_inside_fence_not_matched() {
    let content = r#":::nushell { name: "test" }
```
:::step { name: "not a step" }
```
actual content
:::
"#;
    let tokens = tokenize(content).unwrap();

    let open_count = tokens
        .iter()
        .filter(|t| matches!(t, FenceToken::Open { .. }))
        .count();
    let close_count = tokens
        .iter()
        .filter(|t| matches!(t, FenceToken::Close { .. }))
        .count();

    // Only the nushell open/close should be matched; the :::step inside the code block
    // should be Content, not Open.
    assert_eq!(open_count, 1, "should have only one Open token (nushell)");
    assert_eq!(close_count, 1, "should have only one Close token");
}

#[test]
fn test_unclosed_fence_error() {
    let content = r#":::stage { name: "Setup" }
Some content
"#;
    let result = tokenize(content);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        format!("{err}").contains("Unclosed fence"),
        "error should mention unclosed fence: {err}"
    );
}
