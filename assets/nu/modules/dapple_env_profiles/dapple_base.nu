export-env {
  # Compatible with both old ($nu.home-path) and new ($nu.home-dir) nushell versions
  let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }
  $env.DAPPLE_HOME_DIR = ($home | path join ".dapple")

  # Canonical UUID of the dapple company in rev.companies. Stable across all
  # environments (sandbox, dev, stage, perf, prod). Generated once via
  # `psql -c 'SELECT uuidv7();'` on 2026-05-10 and seeded by Migration 31.
  # See docs/plans/external-api/00-architecture.md §4.1.
  $env.DAPPLE_COMPANY_UUID = "019e132e-2610-7a4a-b64a-77a49e286ac0"
}

# Load environment variables from a .env file
# Only sets variables that aren't already defined in the environment
export def --env load-dotenv [filepath: path] {
    if not ($filepath | path exists) {
        return
    }

    # Parse lines into key-value pairs, filtering those already in env
    let new_vars = open $filepath --raw
        | lines
        | where {|line| $line != "" and not ($line | str starts-with "#")}
        | each {|line|
            let eq_pos = ($line | str index-of "=")
            if $eq_pos >= 0 {
                let key = ($line | str substring 0..<$eq_pos | str trim)
                # Strip a matched surrounding quote pair. NOT a single
                # alternation regex: `str replace -r` replaces only the FIRST
                # match, so `^["']|["']$` stripped the leading quote and left
                # the trailing one in every quoted value.
                let value = ($line
                    | str substring ($eq_pos + 1)..
                    | str trim
                    | str replace -r '^"(.*)"$' "$1"
                    | str replace -r "^'(.*)'$" "$1")
                if ($key | is-not-empty) and not ($key in $env) {
                    {key: $key, value: $value}
                }
            }
        }
        | compact
        | reduce -f {} {|it, acc| $acc | merge {($it.key): $it.value}}

    # Load all new variables at once (outside the each block)
    if ($new_vars | is-not-empty) {
        load-env $new_vars
    }
}

