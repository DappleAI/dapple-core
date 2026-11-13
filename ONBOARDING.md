# Local dev setup — Ubuntu

Sets up the dapple backend on your own machine: a microk8s cluster running the
API, conduit and flow services, with Postgres, Redis and a local image
registry.

Assumes Ubuntu 22.04 or 24.04. On macOS the shape is the same but microk8s runs
inside an OrbStack VM (`assets/nu/k8s/install_orbstack_vm.nu`); on Linux it runs
natively as a snap.

> **Before you start:** look at step 4. Two API keys can't be generated; both
> are optional, but tasks that call Claude or gated Hugging Face models need
> them.

---

## 1. Packages

```bash
sudo apt update && sudo apt install -y \
  docker.io openssl bzip2 libnss3-tools postgresql-client-18 build-essential curl git
```

| package | why |
|---|---|
| `docker.io` | image builds; the dev deploy pushes to the in-cluster registry |
| `openssl` | secret generation, and deriving service-token public keys |
| `bzip2` | the VITS TTS model ships as `.tar.bz2` and `tar` shells out to it |
| `libnss3-tools` | registers the dev CA with Chromium's NSS store — without it browser TLS to `*.kube.dappleai.dev` fails |
| `postgresql-client-18` | `psql` for the token tooling; match the server major (PG 18) |

Add yourself to the docker group and re-login:

```bash
sudo usermod -aG docker $USER && newgrp docker
```

**nushell** is not in Ubuntu's repos. Install from upstream releases or
`cargo install nu`. Check with `nu --version` — the tooling is developed against
0.114.

**Rust** via [rustup](https://rustup.rs). `sqlx-cli` is installed for you in
step 5.

**microk8s**:

```bash
sudo snap install microk8s --classic && sudo usermod -aG microk8s $USER && newgrp microk8s
```

---

## 2. Repo layout

On Linux the tooling expects `~/src/dapple/` (on macOS it's `~/Source/Dapple/`).
This is derived, not configured — put the checkouts here or nothing will find
them:

```bash
mkdir -p ~/src/dapple && cd ~/src/dapple
git clone https://github.com/DappleAI/dapple-core.git dapple-prompt
```

Keep the `dapple-prompt` directory name: the tooling derives
`DAPPLE_PROMPT_WORKSPACE_DIR` from it.

---

## 3. Declare the environment

Every dapple host declares what it is. This is what keeps a laptop from
addressing production and vice versa:

```bash
mkdir -p ~/.dapple && chmod 700 ~/.dapple
printf 'DAPPLE_ENV=sandbox\n' > ~/.dapple/env && chmod 600 ~/.dapple/env
```

---

## 4. API keys you supply

Append these to `~/.dapple/env`. They can't be generated; use your own:

- `ANTHROPIC_API_KEY`: a key from console.anthropic.com. Only needed for
  tasks that call Claude.
- `HF_TOKEN`: a Hugging Face read token. Only needed for gated models.

```
ANTHROPIC_API_KEY=
HF_TOKEN=
```

Setup may warn that `OAUTH_GOOGLE_CLIENT_ID` / `OAUTH_GOOGLE_CLIENT_SECRET` are
missing. Those are for a web front end that isn't part of this repository; the
backend doesn't need them.

Everything else in `assets/env.template` — signing keys, `NEXTAUTH_SECRET`, the
Flagsmith secret key — is generated for you in the next step and written to
`~/.dapple/generated.env`. Don't hand-edit that file; every setup run rewrites
it.

---

## 5. Configure the host

```bash
cd ~/src/dapple/dapple-prompt && nu assets/nu/init/setup.nu --environment sandbox
```

This installs the nu modules and shell profile, generates the secrets file,
installs `sqlx-cli`, and points git at the repo's pre-commit hooks. Open a new
shell afterwards, then confirm the environment resolved:

```bash
nu -c 'print $"($env.DAPPLE_ENV) / ($env.KUBE_HOST) / ($env.DAPPLE_PROMPT_WORKSPACE_DIR)"'
```

Expect `sandbox / kube.dappleai.dev / /home/<you>/src/dapple/dapple-prompt`. If
`DAPPLE_ENV` is empty, `~/.dapple/env` wasn't found — check step 3.

---

## 6. Bring up the cluster

```bash
cd ~/src/dapple/dapple-prompt && nu assets/nu/k8s/setup_microk8s.nu
```

Enables the microk8s addons, deploys the in-cluster registry, dev-dns, and
cert-manager, and installs the dev root CA into your system and NSS trust
stores. Expect a sudo prompt for the CA.

---

## 7. Check host DNS

`setup_microk8s.nu` deploys a dev-dns CoreDNS pod and points your resolver at
it, so `*.kube.dappleai.dev` resolves on the host. That matters more than it
sounds: the API URL, `DATABASE_URL` and the image registry are all addressed
by those names.

Confirm it took:

```bash
getent hosts dapple-prompt-api.kube.dappleai.dev && getent hosts github.com
```

The second lookup matters as much as the first. The rule is scoped to the
`kube.dappleai.dev` suffix; if public DNS broke, the scoping failed and image
pulls will start failing in ways that point nowhere near DNS.

If the first lookup fails, check the pod (`kubectl get pods -n dapple-sandbox -l
app=dev-dns`) and `resolvectl status`. The setup script verifies this itself and
fails loudly, so a silent miss here is unexpected — say so if you hit one.

## 8. Deploy

```bash
cd ~/src/dapple/dapple-prompt && nu assets/nu/k8s/deploy_dapple_prompt_pods.nu
```

Backend services. The first run builds images and takes a while. Then apply
migrations:

```bash
dapple sqlx migrate
```

Check it:

```bash
kubectl get pods -n dapple-sandbox && curl -sk -o /dev/null -w '%{http_code}\n' https://dapple-prompt-api.kube.dappleai.dev/api/health_check
```

A 200 means the whole chain works: DNS → ingress → TLS → API.

---

## Day-to-day

| task | command |
|---|---|
| redeploy one service | `dapple deploy <svc>` |
| rebuild + restart (hostPath services) | `dapple patch <svc> --build` |
| pod logs | `dapple logs <svc> -f` |
| shell in a pod | `dapple shell <svc>` |
| service URL | `dapple url <svc>` |
| apply migrations | `dapple sqlx migrate` |
| after editing a nu module | `nu assets/nu/init/setup.nu --environment sandbox` |

That last one is worth internalising. Deploy **scripts** load modules from your
checkout, so edits take effect immediately. Interactive `dapple …` commands load
the *installed* copy under `$nu.default-config-dir/modules`, so those need the
re-install. A deploy will warn when the two have drifted; it won't block.

---

## When it goes wrong

**`*.kube.dappleai.dev` doesn't resolve** — step 7, or the dev-dns pod is down:
`kubectl get pods -n dapple-sandbox -l app=dev-dns`.

**`x509: certificate signed by unknown authority`, mentioning `local-dev-root-ca`** —
the dev CA rotated and your local copy is stale. The deploy detects and repairs
this itself now; if you're mid-run, re-running is the fix. It'll prompt for sudo
to update the trust store.

**`Cannot find column 'SOMETHING'`** — a variable the shell profile is expected
to provide is missing. Check `~/.dapple/env` exists and declares `DAPPLE_ENV`.

**A red banner about addressing the wrong cluster** — the guard found a
production-declared host resolving dev addresses (or vice versa). It blanks the
offending variables rather than letting a tool write to the wrong database.
Believe it and fix the cause; don't work around it.

**`dapple` commands behave oddly after a `git pull`** — stale installed modules.
`nu assets/nu/init/setup.nu --environment sandbox`.

**hostPath services ignore `dapple patch`/`quick`** — they run binaries from a
mounted host target dir, not the image. Use `patch --build`.
