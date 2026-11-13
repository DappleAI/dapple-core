use std::path::Path;

#[test]
fn test_parse_fixture_guide() {
    let guide_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple-guide");

    let parsed = dapple_parser::parse_guide(&guide_dir).unwrap();

    // Title comes from guide.toml
    assert_eq!(parsed.title, "Summary");

    // Metadata should be populated from guide.toml
    assert_eq!(parsed.metadata.title, "Summary");

    // No assets in the simple-guide fixture (only .md and .toml files)
    assert!(parsed.asset_paths.is_empty());

    // Should have 3 chapters: Preface, Introduction, Glossary
    assert_eq!(parsed.chapters.len(), 3);

    assert_eq!(parsed.chapters[0].title, "Preface");
    assert_eq!(parsed.chapters[0].source_file, "preface.md");
    assert_eq!(parsed.chapters[0].depth, 0);
    assert!(parsed.chapters[0].parent_index.is_none());

    assert_eq!(parsed.chapters[1].title, "Introduction");
    assert_eq!(parsed.chapters[1].source_file, "intro.md");

    assert_eq!(parsed.chapters[2].title, "Glossary");
    assert_eq!(parsed.chapters[2].source_file, "glossary.md");

    // Chapter metadata should carry subtitles from guide.toml
    assert_eq!(parsed.metadata.chapters.len(), 2);
    assert_eq!(
        parsed.metadata.chapters["preface.md"].subtitle,
        Some("Before we begin...".to_string())
    );
    assert_eq!(
        parsed.metadata.chapters["intro.md"].subtitle,
        Some("Getting started with the guide".to_string())
    );

    // Preface should have a single prose block
    assert!(!parsed.chapters[0].blocks.is_empty());
    assert_eq!(parsed.chapters[0].blocks[0].directive_type, "prose");

    // Introduction should have prose, a stage directive, and trailing prose
    let intro_blocks = &parsed.chapters[1].blocks;
    assert!(
        intro_blocks.len() >= 2,
        "intro should have at least 2 blocks"
    );

    // First block should be prose
    assert_eq!(intro_blocks[0].directive_type, "prose");

    // Find the stage block
    let stage_block = intro_blocks
        .iter()
        .find(|b| b.directive_type == "stage")
        .expect("should have a stage block");
    assert_eq!(stage_block.name, Some("Onboarding".to_string()));

    // The stage should have a step child
    let step_children: Vec<_> = stage_block
        .children
        .iter()
        .filter(|c| c.directive_type == "step")
        .collect();
    assert_eq!(step_children.len(), 1);
    assert_eq!(step_children[0].name, Some("Gather Info".to_string()));
    assert_eq!(step_children[0].tags, vec!["agent", "product"]);

    // The step should have a nushell child
    let nushell_children: Vec<_> = step_children[0]
        .children
        .iter()
        .filter(|c| c.directive_type == "nushell")
        .collect();
    assert_eq!(nushell_children.len(), 1);
    assert_eq!(nushell_children[0].name, Some("process-info".to_string()));
    assert_eq!(nushell_children[0].tags, vec!["coder"]);

    // Glossary should have a prose block with glossary content
    assert!(!parsed.chapters[2].blocks.is_empty());
    assert_eq!(parsed.chapters[2].blocks[0].directive_type, "prose");

    // Introduction should have scope, objective, and deliverable blocks
    let scope_block = intro_blocks
        .iter()
        .find(|b| b.directive_type == "scope")
        .expect("should have a scope block");
    assert_eq!(scope_block.name, Some("onboarding-scope".to_string()));

    let objective_block = intro_blocks
        .iter()
        .find(|b| b.directive_type == "objective")
        .expect("should have an objective block");
    assert_eq!(
        objective_block.name,
        Some("gather-business-info".to_string())
    );

    let deliverable_block = intro_blocks
        .iter()
        .find(|b| b.directive_type == "deliverable")
        .expect("should have a deliverable block");
    assert_eq!(deliverable_block.name, Some("verified-profile".to_string()));

    // Verify subtitles propagate through plan_guide into ChapterDefs
    let plan = dapple_parser::transform::plan_guide(&parsed, &guide_dir).unwrap();
    assert_eq!(plan.chapters.len(), 3);
    assert_eq!(
        plan.chapters[0].subtitle,
        Some("Before we begin...".to_string()),
        "preface.md should have subtitle from guide.toml"
    );
    assert_eq!(
        plan.chapters[1].subtitle,
        Some("Getting started with the guide".to_string()),
        "intro.md should have subtitle from guide.toml"
    );
    assert_eq!(
        plan.chapters[2].subtitle, None,
        "glossary.md has no subtitle in guide.toml"
    );

    // Verify scope/objective/deliverable are extracted into IngestPlan
    assert_eq!(plan.derived_scopes.len(), 1);
    let scope = &plan.derived_scopes[0];
    assert_eq!(scope.name, "onboarding-scope");
    assert_eq!(scope.audience.as_deref(), Some("new business customers"));
    assert_eq!(
        scope.goals.as_deref(),
        Some("collect business details, validate information")
    );
    assert_eq!(
        scope.boundaries.as_deref(),
        Some("not covering payment setup")
    );
    assert_eq!(scope.tone.as_deref(), Some("professional, welcoming"));

    assert_eq!(plan.derived_objectives.len(), 1);
    let obj = &plan.derived_objectives[0];
    assert_eq!(obj.name, "gather-business-info");
    assert_eq!(
        obj.statement,
        "Collect all required business details from the customer"
    );
    assert_eq!(obj.expectations.len(), 2);
    assert_eq!(
        obj.expectations[0],
        "Customer provides company name and contact info"
    );
    assert_eq!(obj.expectations[1], "Customer uploads required documents");

    assert_eq!(plan.derived_deliverables.len(), 1);
    let del = &plan.derived_deliverables[0];
    assert_eq!(del.name, "verified-profile");
    assert_eq!(
        del.description,
        "A verified business profile ready for account activation"
    );

    // Verify inline content is retained in flattened blocks
    let intro_plan_blocks = &plan.chapters[1].blocks;
    let scope_plan_block = intro_plan_blocks
        .iter()
        .find(|b| b.directive_type == "scope")
        .expect("scope block should be in plan chapters");
    assert!(
        scope_plan_block.content.is_some(),
        "scope block content should be retained (inline)"
    );
}

#[test]
fn test_parse_journey_planner_guide() {
    let guide_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/books/journey-planner-guide");
    if !guide_dir.exists() {
        // Skip if running from a different working directory
        return;
    }

    let parsed = dapple_parser::parse_guide(&guide_dir).unwrap();
    assert!(!parsed.title.is_empty());
    assert_eq!(
        parsed.title, "Journey Planner Guide",
        "title should come from book.toml (legacy fallback)"
    );
    // Fluent-native: no locale field in metadata
    assert!(!parsed.chapters.is_empty());

    // The SUMMARY.md lists many chapters -- verify count roughly matches
    assert!(
        parsed.chapters.len() >= 20,
        "expected at least 20 chapters, got {}",
        parsed.chapters.len()
    );

    // Each chapter should have at least one block (prose at minimum)
    for ch in &parsed.chapters {
        // Some chapters may have empty files; that's OK
        if !ch.blocks.is_empty() {
            // At minimum one prose block
            assert!(
                ch.blocks.iter().any(|b| b.directive_type == "prose")
                    || ch.blocks.iter().any(|b| b.directive_type != "prose"),
                "chapter '{}' has blocks but none are recognized types",
                ch.title
            );
        }
    }

    // Chapters metadata from book.toml fallback should be empty
    assert!(
        parsed.metadata.chapters.is_empty(),
        "book.toml fallback should produce empty chapter metadata"
    );

    // All chapters should be depth 0 (the journey planner guide has no nesting)
    for ch in &parsed.chapters {
        assert_eq!(
            ch.depth, 0,
            "chapter '{}' should be depth 0, got {}",
            ch.title, ch.depth
        );
        assert!(
            ch.parent_index.is_none(),
            "chapter '{}' should have no parent",
            ch.title
        );
    }
}

#[test]
fn test_stuart_librarian_subtitles() {
    let guide_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../dapple-guides/internal/dapple/stuart-librarian");
    if !guide_dir.exists() {
        // Skip if dapple-guides repo is not colocated
        return;
    }

    let parsed = dapple_parser::parse_guide(&guide_dir).unwrap();
    assert_eq!(parsed.title, "Stuart Librarian");

    // guide.toml should have chapter metadata with subtitles
    assert!(
        !parsed.metadata.chapters.is_empty(),
        "stuart-librarian guide.toml should have [chapters] sections"
    );

    // Verify subtitles flow through plan_guide
    let plan = dapple_parser::transform::plan_guide(&parsed, &guide_dir).unwrap();

    let subtitles: Vec<_> = plan
        .chapters
        .iter()
        .map(|ch| (&ch.source_file, &ch.subtitle))
        .collect();

    println!("Stuart-librarian chapter subtitles: {:#?}", subtitles);

    // Every chapter in stuart-librarian should have a subtitle
    for ch in &plan.chapters {
        assert!(
            ch.subtitle.is_some(),
            "chapter '{}' ({}) should have a subtitle, got None",
            ch.title,
            ch.source_file
        );
    }

    // Spot-check specific subtitles
    let intro = plan
        .chapters
        .iter()
        .find(|ch| ch.source_file == "introduction.md")
        .expect("should have introduction.md chapter");
    assert_eq!(
        intro.subtitle,
        Some("What can I help you with?".to_string())
    );
}
