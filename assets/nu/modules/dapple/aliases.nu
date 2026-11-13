# Service alias to deployment name mapping
#
# This module provides consistent service name resolution across all dapple CLI commands.
# Users can use short aliases (e.g., "api") instead of full deployment names.

# Resolve a service alias to its full deployment name
export def resolve-service [alias: string]: nothing -> string {
    match $alias {
        "api" => "dapple-prompt-api"
        "external-api" => "dapple-prompt-external-api"
        "conduit" => "dapple-prompt-conduit"
        "flow" => "dapple-prompt-flow"
        "rust-docs" | "docs" => "rust-docs"
        "ui" => "dapple-ui"
        "landing" => "dapple-web-landing-page"
        "postgres" => "prompt-db"
        "redis" => "dapple-redis"
        _ => $alias  # Pass through full names
    }
}

# List of all known service aliases
export def list-aliases []: nothing -> list<record<alias: string, deployment: string>> {
    [
        { alias: "api", deployment: "dapple-prompt-api" }
        { alias: "external-api", deployment: "dapple-prompt-external-api" }
        { alias: "conduit", deployment: "dapple-prompt-conduit" }
        { alias: "flow", deployment: "dapple-prompt-flow" }
        { alias: "rust-docs", deployment: "rust-docs" }
        { alias: "docs", deployment: "rust-docs" }
        { alias: "ui", deployment: "dapple-ui" }
        { alias: "landing", deployment: "dapple-web-landing-page" }
        { alias: "postgres", deployment: "prompt-db" }
        { alias: "redis", deployment: "dapple-redis" }
    ]
}

# Get aliases for buildable services (Rust services + docs)
export def buildable-services []: nothing -> list<string> {
    ["api" "external-api" "conduit" "flow" "rust-docs"]
}
