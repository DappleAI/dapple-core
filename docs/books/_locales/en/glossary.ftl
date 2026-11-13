# Glossary terms for dapple-prompt documentation
# Language: English (en)

# Core Execution Model
term-task = Task
term-task-def = A single unit of work implemented as a small script.
    Tasks have clear inputs, outputs, and exit behavior.

term-step = Setup Step
term-step-def = A dynamic sub-interaction within a Stage (one round-trip
    of work plus any operator input) before the user moves on. The
    canonical shape is generate, pick, refine: a task generates
    candidates, the operator picks one, and a later step refines the
    pick. Multi-step stages are the norm.

term-stage = Stage
term-stage-def = A static form or pane the user lands on; advancing to
    the next stage is the real navigation. A Stage holds one or more
    Setup Steps, the sub-interactions that happen on that pane.

term-playbook = Playbook
term-playbook-def = A user-facing orchestration that combines
    Stages into a full flow.

term-playbook-instance = Playbook Instance / Run
term-playbook-instance-def = A single execution of a Playbook.
    The runtime creates one instance per user-triggered execution.

term-journey = Journey
term-journey-def = A product-level story or experience composed of
    one or more Playbooks, UX decisions, branching rules, and
    longer-lived state.

# Briefs & Packs
term-brief = Brief
term-brief-def = A versioned container for UI elements (inputs,
    outputs, markdown) used in debrief interviews.

term-brief-group = BriefGroup
term-brief-group-def = A logical grouping of related Briefs that
    must be completed atomically.

term-brief-pack = Brief Pack
term-brief-pack-def = A package of Playbooks, Briefs, prompts,
    example inputs, and metadata intended for distribution.

term-pack-manifest = Pack Manifest
term-pack-manifest-def = The manifest file (YAML/JSON) at the root
    of a Brief Pack describing playbooks, prompts, examples,
    required binaries, and metadata.

# Artifacts & Files
term-artifact = Artifact
term-artifact-def = Any file or structured output produced by a
    Task (e.g., a Markdown resume, a PDF, a JSON metadata file).

term-artifact-json = artifact.json
term-artifact-json-def = The minimal contract file a Task should
    write when producing a primary artifact.

# Prompts & LLM
term-prompt = Prompt
term-prompt-def = A text template (often with placeholders) sent
    to an LLM to drive extraction, normalization, or content
    generation.

term-prompt-template = Prompt Template
term-prompt-template-def = A reusable prompt file that contains
    placeholders like {{user_text}} or {{input_json}} intended
    for substitution at runtime.

term-conduit = Conduit
term-conduit-def = The runtime bridge that routes prompt templates
    and substituted prompts to a configured LLM provider.

term-overlay = Overlay
term-overlay-def = A runtime configuration bundle that overrides
    prompt routing/conduit behavior without editing the prompt.

# Authentication
term-pat = Personal Access Token (PAT)
term-pat-def = A user-scoped, hash-stored token for programmatic
    API access. Prefixed with dpl_ for identification.

term-jwt = JWT (JSON Web Token)
term-jwt-def = A self-contained token that encodes claims and can
    be validated without database lookup.

term-service-token = Service Token
term-service-token-def = An Ed25519-signed token for service-to-service
    authentication between internal services.
