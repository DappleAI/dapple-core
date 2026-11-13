Simple Nushell Task (template)

# TASK: collect-personal-data
# Inputs: JSON on stdin with keys: name,email,work_history
# Outputs: writes `artifact.json` with path to input.json

```bash
#!/usr/bin/env bash
set -euo pipefail
OUT_DIR=${OUT_DIR:-./out}
mkdir -p "$OUT_DIR"
cat - > "$OUT_DIR/input.json"
cat <<EOF > "$OUT_DIR/artifact.json"
{ "path": "$OUT_DIR/input.json", "type": "application/json" }
EOF
```
