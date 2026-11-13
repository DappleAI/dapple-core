# Where environment configuration lives, for every environment.
#
# Two files, one role each, same paths everywhere:
#
#   ~/.dapple/env            hand-authored. Secrets and the DAPPLE_ENV
#                            declaration. Never machine-rewritten.
#   ~/.dapple/generated.env  machine-written, rewritten by every deploy.
#                            Hand edits do not survive, which is why it is
#                            named that.
#
# This replaces four files with overlapping roles: ~/.env and ~/.env.prod [which
# were byte-identical copies on the prod host, because install read one path
# while the shell read the other], ~/.env.dapple, and ~/.dapple/prod.env. The
# duplication was not a decision; it was two consumers looking at different
# paths and someone reconciling them with cp.
#
# ~/.env is deliberately NOT one of these. dapple-suite's binaries used to
# hardcode it, and a file that both a shell profile and a Rust dotenv loader
# claim — with opposite precedence rules — is how a prod host ends up addressing
# the dev cluster while every layer believes it won.

export const DAPPLE_DIR_NAME = ".dapple"
export const USER_ENV_NAME = "env"
export const GENERATED_ENV_NAME = "generated.env"

# The key inside ~/.dapple/env that declares the host's environment.
#
# It lives in the env file rather than a file of its own because a third file is
# the proliferation this cleanup exists to end. But it is a DECLARATION, not
# proof: env files get copied between machines, which is exactly how the prod
# host acquired dev configuration. Callers must verify it against something
# real — `dapple_env::assert_declared` below, and the installer's --env flag.
export const ENV_KEY = "DAPPLE_ENV"

export def dapple_dir [home: path]: nothing -> path {
    $home | path join $DAPPLE_DIR_NAME
}

export def user_env_path [home: path]: nothing -> path {
    (dapple_dir $home) | path join $USER_ENV_NAME
}

export def generated_env_path [home: path]: nothing -> path {
    (dapple_dir $home) | path join $GENERATED_ENV_NAME
}

# Read one key out of an env file without loading the whole thing into the
# environment. Used to learn DAPPLE_ENV before deciding anything else.
export def read_key [file: path, key: string]: nothing -> string {
    if not ($file | path exists) {
        return ""
    }
    let hit = (
        open $file --raw
        | lines
        | where {|line| ($line | str trim | str starts-with $"($key)=") }
        | last
    )
    if ($hit | is-empty) {
        return ""
    }
    $hit | str trim | str substring (($key | str length) + 1).. | str trim
        | str replace -r '^"(.*)"$' "$1"
        | str replace -r "^'(.*)'$" "$1"
}

# Parse a whole env file into a record of key -> value.
#
# MUST mirror `load-dotenv` in dapple_base.nu line for line — same `str
# index-of "="` split, same trim, same matched-quote-pair strip. Its only
# consumer is the conflict report, and a report that parses differently from
# the loader is worse than no report: it would flag pairs the shell resolves
# identically [a trailing space, or one side quoted] and stay quiet on ones it
# doesn't. Not imported from dapple_base because `use`ing that module fires its
# export-env as a side effect.
export def read_pairs [file: path]: nothing -> record {
    if not ($file | path exists) {
        return {}
    }
    open $file --raw
    | lines
    | where {|line| $line != "" and not ($line | str starts-with "#")}
    | each {|line|
        let eq_pos = ($line | str index-of "=")
        if $eq_pos >= 0 {
            let key = ($line | str substring 0..<$eq_pos | str trim)
            let value = ($line
                | str substring ($eq_pos + 1)..
                | str trim
                | str replace -r '^"(.*)"$' "$1"
                | str replace -r "^'(.*)'$" "$1")
            if ($key | is-not-empty) {
                {key: $key, value: $value}
            }
        }
    }
    | compact
    | reduce -f {} {|it, acc| $acc | merge {($it.key): $it.value}}
}

# Resolve the declared environment for a host.
#
# Order: an explicit DAPPLE_ENV in the process environment wins [so a one-off
# `DAPPLE_ENV=sandbox nu ...` works], then the declaration in ~/.dapple/env.
# Returns "" when nothing declares one — callers decide whether that is fatal.
export def declared_environment [home: path, process_env: record]: nothing -> string {
    let from_process = ($process_env | get -o $ENV_KEY | default "" | str trim)
    if ($from_process | is-not-empty) {
        return $from_process
    }
    read_key (user_env_path $home) $ENV_KEY | str trim
}

# The legacy paths this layout replaces, newest role first.
#
# Kept only so the installer can MIGRATE them. Nothing reads these at runtime:
# the transition is a hard cut, because a permanent fallback chain is the same
# ambiguity that produced the mess.
export const LEGACY_USER_ENV_NAMES = [".env.prod" ".env"]
export const LEGACY_GENERATED_ENV_NAMES = [".env.dapple"]

# Move a host's legacy env files to the new layout, once.
#
# The transition is a hard cut — nothing reads the old paths afterwards — so
# this has to run before the new profile can see anything, which is why every
# installer calls it. It is idempotent: once ~/.dapple/env exists it does
# nothing.
#
# Symlinks are RESOLVED rather than moved. The prod host currently has ~/.env
# symlinked to ~/.env.prod as a stopgap, and carrying that link into the new
# layout would preserve exactly the ambiguity this replaces.
#
# `declared_env` is stamped in as DAPPLE_ENV if the migrated file doesn't
# already declare one, so a migrated host is never left undeclared.
export def --env migrate_legacy_files [home: path, declared_env: string] {
    let dir = (dapple_dir $home)
    mkdir $dir

    let user_target = (user_env_path $home)
    if not ($user_target | path exists) {
        let source = (
            $LEGACY_USER_ENV_NAMES
            | each {|name| $home | path join $name }
            | where {|p| $p | path exists }
            | first
        )
        if ($source | is-not-empty) {
            # `path expand` follows the symlink to the real file.
            let real = ($source | path expand)
            print $"migrating ($real) -> ($user_target)"
            cp $real $user_target
            chmod 600 $user_target
        } else {
            print $"creating ($user_target)"
            "" | save --force $user_target
            chmod 600 $user_target
        }
    }

    # Stamp the declaration if the file doesn't carry one.
    if (read_key $user_target $ENV_KEY | is-empty) {
        print $"declaring ($ENV_KEY)=($declared_env) in ($user_target)"
        let body = (open $user_target --raw)
        $"($ENV_KEY)=($declared_env)\n($body)" | save --force $user_target
    }

    let gen_target = (generated_env_path $home)
    if not ($gen_target | path exists) {
        let source = (
            $LEGACY_GENERATED_ENV_NAMES
            | each {|name| $home | path join $name }
            | where {|p| $p | path exists }
            | first
        )
        if ($source | is-not-empty) {
            print $"migrating ($source | path expand) -> ($gen_target)"
            cp ($source | path expand) $gen_target
            chmod 600 $gen_target
        }
    }

    report_conflicts $user_target $gen_target
}

# Name every key the two files define differently.
#
# The migration relocates two files whose relative precedence was NOT the same
# as the new pair's, so any key present in both can change value — and
# load-dotenv reports nothing when one does. Silence IS the failure mode here:
# the original symptom was a workstation quietly swapping OAUTH_GOOGLE_CLIENT_ID
# for the other file's copy, which deploy_dapple_web_pods.nu then stamped into
# the dapple-ui-auth secret on a green deploy.
#
# Names only. These files are full of secrets and this output is not.
def report_conflicts [user_target: path, gen_target: path] {
    let user_pairs = (read_pairs $user_target)
    let gen_pairs = (read_pairs $gen_target)

    let conflicts = (
        $user_pairs
        | columns
        | where {|k| ($k in $gen_pairs) and (($user_pairs | get $k) != ($gen_pairs | get $k)) }
        | sort
    )
    if ($conflicts | is-empty) {
        return
    }

    # The hand-authored file wins in every environment — see the load-order
    # block in dapple_env.nu. No lookup here: one rule, stated once.
    let noun = if ($conflicts | length) == 1 { "key is" } else { "keys are" }
    print $"(ansi yellow_bold)⚠  ($conflicts | length) ($noun) defined differently in both env files(ansi reset)"
    for key in $conflicts { print $"     ($key)" }
    print $"   ($user_target) wins — it is hand-authored, and that outranks generated.env everywhere."
    print $"   If the value you want is the one in ($gen_target), move it across now:"
    print "   nothing warns about this again, and generated.env is rewritten by every deploy."
}
