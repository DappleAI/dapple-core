# Compatibility shim.
#
# `dapple_sandbox` used to BE the environment: it hardcoded the dev cluster's
# hostnames, database URL and password, and workspace layout. A host was
# "sandbox" because this module is what its shell loaded — which is how the
# production box ended up exporting the dev database URL for weeks.
#
# The environment now comes from DAPPLE_ENV in ~/.dapple/env and the table in
# environments.nu, so this is a plain alias for dapple_env. It stays only so
# workstations that haven't re-run the installer keep working; delete it once
# they have.
#
# NOTE: loading this no longer forces the sandbox environment. If you are on a
# host declared as prod, you get prod. That is the point.

export use (path self dapple_env.nu) *

export-env {
    if not (overlay list | any {|o| $o == "dapple_env"}) {
        overlay use dapple_env
    }
}
