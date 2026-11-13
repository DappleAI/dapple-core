#[test]
fn test_parse_simple_summary() {
    let content = r#"
# Summary

[Preface](./preface.md)

- [Introduction](./intro.md)

# Getting Started

- [Glossary](./glossary.md)
- [Concepts](./concepts.md)
"#;
    let chapters = dapple_parser::summary::parse_summary(content).unwrap();
    assert_eq!(chapters.len(), 4);

    assert_eq!(chapters[0].title, "Preface");
    assert_eq!(chapters[0].source_file, "preface.md");
    assert_eq!(chapters[0].depth, 0);
    assert!(chapters[0].parent_index.is_none());

    assert_eq!(chapters[1].title, "Introduction");
    assert_eq!(chapters[1].source_file, "intro.md");

    assert_eq!(chapters[2].title, "Glossary");
    assert_eq!(chapters[2].source_file, "glossary.md");

    assert_eq!(chapters[3].title, "Concepts");
    assert_eq!(chapters[3].source_file, "concepts.md");
}

#[test]
fn test_parse_nested_summary() {
    let content = r#"
# Summary

- [Chapter 1](./ch1.md)
  - [Section 1.1](./ch1/sec1.md)
  - [Section 1.2](./ch1/sec2.md)
    - [Subsection 1.2.1](./ch1/sec2/sub1.md)
- [Chapter 2](./ch2.md)
"#;
    let chapters = dapple_parser::summary::parse_summary(content).unwrap();
    assert_eq!(chapters.len(), 5);

    assert_eq!(chapters[0].depth, 0);
    assert!(chapters[0].parent_index.is_none());

    assert_eq!(chapters[1].depth, 1);
    assert_eq!(chapters[1].parent_index, Some(0));

    assert_eq!(chapters[2].depth, 1);
    assert_eq!(chapters[2].parent_index, Some(0));

    assert_eq!(chapters[3].depth, 2);
    assert_eq!(chapters[3].parent_index, Some(2));

    assert_eq!(chapters[4].depth, 0);
    assert!(chapters[4].parent_index.is_none());
}
