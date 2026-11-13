export-env {
  let home = if "home-dir" in ($nu | columns) { $nu.home-dir } else { $nu.home-path }

  # Ensure DAPPLE_HOME_DIR is set (normally from dapple_base, but we need it here)
  if not ("DAPPLE_HOME_DIR" in $env) {
    $env.DAPPLE_HOME_DIR = ($home | path join ".dapple")
  }

  # Same defensive pattern: dapple_base.nu's export-env doesn't propagate
  # through the overlay chain reliably, so re-declare the dapple company UUID
  # here. Source of truth is dapple_base.nu; keep in sync if it ever changes
  # (which it shouldn't — this UUID is stable across all environments).
  if not ("DAPPLE_COMPANY_UUID" in $env) {
    $env.DAPPLE_COMPANY_UUID = "019e132e-2610-7a4a-b64a-77a49e286ac0"
  }

  $env.KUBE_HOST = "kube.dappleai.dev"
  $env.KUBE_REGISTRY_HOST = "registry.kube.dappleai.dev"
  $env.KUBE_DAPPLE_NAMESPACE = "dapple-sandbox"
  $env.KUBE_ROOT_CA_CERT_NAME = "local-dev-root-ca.crt"

  $env.KUBE_CERTS_DIR = ($env.DAPPLE_HOME_DIR | path join "certs")
  $env.DOCKER_DAPPLE_TAG_LATEST = "dapple-latest"
  $env.DOCKER_DIR = ($home | path join ".docker")

  # Enable BuildKit for all `docker build` invocations so Dockerfiles
  # can use `RUN --mount=type=cache,...` and other BuildKit-specific
  # features. Docker Desktop ships with BuildKit but the classic
  # `docker build` CLI still defaults to the legacy builder unless
  # DOCKER_BUILDKIT=1 is set explicitly.
  $env.DOCKER_BUILDKIT = "1"

  # On Linux, default to native dockerd socket. Docker Desktop installs itself
  # as the default context (unix://~/.docker/desktop/docker.sock) which breaks
  # all `docker` commands when Desktop isn't running. Explicitly point to the
  # native daemon unless the user opts in to Docker Desktop.
  let is_macos = ($nu.os-info.name | str contains "mac")
  let uses_desktop = $is_macos or (($env | get -o DAPPLE_DOCKER_DESKTOP | default "") | str trim) == "true"
  if not $uses_desktop {
    $env.DOCKER_HOST = "unix:///var/run/docker.sock"
  }
}

# kubectl wrapper that dispatches per platform:
#
# - On Linux, microk8s installs natively as a snap and ships its own
#   embedded kubectl. Routing `kubectl` through `^microk8s kubectl`
#   avoids needing a separate kubectl install or per-host kubeconfig.
#
# - On macOS under OrbStack, microk8s lives inside the VM. The host's
#   brew-installed kubectl talks to it via the kubeconfig produced by
#   install_orbstack_vm.nu (current-context: microk8s, server pointing
#   at the OrbStack VM IP). We do NOT route through `^microk8s kubectl`
#   because on macOS that resolves to a multipass-launcher wrapper that
#   no longer applies.
#
# `--wrapped` makes nushell pass all args through verbatim so flags
# like `-n`, `--context`, `-o yaml`, etc. reach the inner kubectl.
export def --wrapped kubectl [...args] {
    if ($nu.os-info.name | str contains "mac") {
        ^kubectl ...$args
    } else {
        ^microk8s kubectl ...$args
    }
}

