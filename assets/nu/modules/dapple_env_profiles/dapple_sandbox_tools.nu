# Compatibility shim — see dapple_sandbox.nu.
export use (path self dapple_env.nu) *

export-env {
    if not (overlay list | any {|o| $o == "dapple_env"}) {
        overlay use dapple_env
    }
}
