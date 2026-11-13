# Project Plan Document Template

This document defines the structure for project plans under `docs/plans/`. Plans serve as exhaustive guides for agents to implement features without spending context on codebase exploration.

---

## Plan Document Structure

Each plan document should contain:

### 1. Header
- **Title**: Descriptive name
- **SHA-1**: The commit hash this plan builds from
- **Status**: Draft | Ready | In Progress | Complete
- **Created**: Date
- **Author**: Who wrote the plan

### 2. Summary
Brief (2-3 sentences) description of what this plan accomplishes.

### 3. Current State
Detailed description of how things work today. Include:
- File paths to relevant code
- Key structs, traits, functions with code snippets for critical pieces
- Data flow descriptions
- Current limitations or problems

### 4. Desired State
What the system should look like after implementation. Include:
- Architecture diagrams (ASCII or mermaid)
- New structs/traits/functions needed
- Changed behaviors
- Success criteria

### 5. Implementation Steps
Ordered list of concrete steps to implement the change. Each step should:
- Be independently verifiable
- Reference specific files to modify
- Include code snippets where helpful

### 6. Files to Modify
Table of files and what changes in each.

### 7. Verification
How to test the implementation is correct:
- Commands to run
- Expected outputs
- Test cases to add

### 8. Open Questions
Any unresolved decisions that need input.
