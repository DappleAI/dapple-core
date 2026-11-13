use dapple_parser::block::classify_blocks;
use dapple_parser::fence::tokenize;

#[test]
fn test_classify_prose_and_directive() {
    let content = r#"Intro prose.

:::nushell { name: "my-task", tags: [coder] }
def main [] { { payload: {}, signals: {} } }
:::

Outro prose."#;

    let tokens = tokenize(content).unwrap();
    let blocks = classify_blocks(tokens, &["nushell"]).unwrap();

    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].directive_type, "prose");
    assert_eq!(blocks[1].directive_type, "nushell");
    assert_eq!(blocks[1].name, Some("my-task".to_string()));
    assert_eq!(blocks[1].tags, vec!["coder"]);
    assert_eq!(blocks[2].directive_type, "prose");
}

#[test]
fn test_classify_nested_blocks() {
    let content = r#":::stage { name: "Setup" }
:::step { name: "Gather" }
Content here
:::
:::
"#;

    let tokens = tokenize(content).unwrap();
    let blocks = classify_blocks(tokens, &["stage", "step"]).unwrap();

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].directive_type, "stage");
    assert_eq!(blocks[0].name, Some("Setup".to_string()));
    assert_eq!(blocks[0].children.len(), 1);
    assert_eq!(blocks[0].children[0].directive_type, "step");
    assert_eq!(blocks[0].children[0].name, Some("Gather".to_string()));
    assert!(blocks[0].children[0].content.contains("Content here"));
}

#[test]
fn test_classify_deeply_nested_blocks() {
    let content = r#":::stage { name: "Onboarding" }

:::step { name: "Gather Info", tags: [agent, product] }

Please provide your business details.

:::nushell { name: "process-info", tags: [coder] }
def main [] { { payload: {}, signals: {} } }
:::

:::

:::
"#;

    let tokens = tokenize(content).unwrap();
    let blocks = classify_blocks(tokens, &["stage", "step", "nushell"]).unwrap();

    assert_eq!(blocks.len(), 1, "should have one top-level stage block");
    let stage = &blocks[0];
    assert_eq!(stage.directive_type, "stage");
    assert_eq!(stage.name, Some("Onboarding".to_string()));

    // The stage should contain the step as a child
    let step_children: Vec<_> = stage
        .children
        .iter()
        .filter(|c| c.directive_type == "step")
        .collect();
    assert_eq!(step_children.len(), 1, "stage should contain one step");

    let step = step_children[0];
    assert_eq!(step.name, Some("Gather Info".to_string()));
    assert_eq!(step.tags, vec!["agent", "product"]);

    // The step should contain the nushell task as a child
    let nushell_children: Vec<_> = step
        .children
        .iter()
        .filter(|c| c.directive_type == "nushell")
        .collect();
    assert_eq!(
        nushell_children.len(),
        1,
        "step should contain one nushell task"
    );

    let nushell = nushell_children[0];
    assert_eq!(nushell.name, Some("process-info".to_string()));
    assert_eq!(nushell.tags, vec!["coder"]);
    assert!(nushell.content.contains("def main"));
}

#[test]
fn test_classify_prose_between_directives() {
    let content = r#"First prose block.

:::nushell { name: "task1" }
code1
:::

Middle prose block.

:::nushell { name: "task2" }
code2
:::

Last prose block."#;

    let tokens = tokenize(content).unwrap();
    let blocks = classify_blocks(tokens, &["nushell"]).unwrap();

    assert_eq!(blocks.len(), 5);
    assert_eq!(blocks[0].directive_type, "prose");
    assert_eq!(blocks[1].directive_type, "nushell");
    assert_eq!(blocks[1].name, Some("task1".to_string()));
    assert_eq!(blocks[2].directive_type, "prose");
    assert_eq!(blocks[3].directive_type, "nushell");
    assert_eq!(blocks[3].name, Some("task2".to_string()));
    assert_eq!(blocks[4].directive_type, "prose");
}

#[test]
fn test_classify_directive_hyphen_normalization() {
    let content = r#":::brief-pack { name: "test-pack" }
Some content.
:::
"#;

    let tokens = tokenize(content).unwrap();
    let blocks = classify_blocks(tokens, &["brief-pack"]).unwrap();

    assert_eq!(blocks.len(), 1);
    assert_eq!(
        blocks[0].directive_type, "brief_pack",
        "hyphens should be normalized to underscores"
    );
}

#[test]
fn test_classify_empty_content() {
    let content = "";
    let tokens = tokenize(content).unwrap();
    let blocks = classify_blocks(tokens, &[]).unwrap();
    assert!(blocks.is_empty());
}
