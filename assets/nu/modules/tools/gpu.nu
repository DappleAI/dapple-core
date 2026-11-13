# GPU detection and configuration module for ONNX Runtime deployments
#
# Detects available GPU hardware and returns configuration for:
# - ONNX Runtime execution providers
# - Kubernetes device plugins and resource requests
# - Docker base images
# - Required environment variables

# Detect available GPU hardware and return full configuration
export def detect []: nothing -> record {
    let gpu_type = detect_type
    let gpu_arch = detect_arch $gpu_type

    {
        type: $gpu_type,                           # "amd" | "nvidia" | "apple" | "none"
        arch: $gpu_arch,                           # "gfx1102", "sm_89", "m1", etc.
        onnx_ep: (get_onnx_ep $gpu_type),          # ONNX execution provider
        device_plugin: (get_device_plugin $gpu_type),
        k8s_resource: (get_k8s_resource $gpu_type),
        docker_base: (get_docker_base $gpu_type),
        env_vars: (get_env_vars $gpu_type $gpu_arch),
    }
}

# Detect the GPU type present on the system
def detect_type []: nothing -> string {
    # AMD: Check rocminfo
    if (which rocminfo | is-not-empty) {
        let r = (do { rocminfo } | complete)
        if $r.exit_code == 0 and ($r.stdout | str contains "gfx") {
            return "amd"
        }
    }

    # NVIDIA: Check nvidia-smi
    if (which nvidia-smi | is-not-empty) {
        let r = (do { nvidia-smi } | complete)
        if $r.exit_code == 0 {
            return "nvidia"
        }
    }

    # Apple Silicon
    if ($nu.os-info.name == "macos") and ((^uname -m | str trim) == "arm64") {
        return "apple"
    }

    "none"
}

# Detect the specific GPU architecture
def detect_arch [gpu_type: string]: nothing -> string {
    match $gpu_type {
        "amd" => {
            # Extract gfx version from rocminfo output
            let info = (rocminfo | lines | find "gfx" | first | default "unknown")
            $info | str trim
        }
        "nvidia" => {
            # Get compute capability from nvidia-smi
            let info = (nvidia-smi --query-gpu=compute_cap --format=csv,noheader | str trim)
            $"sm_($info | str replace '.' '_')"
        }
        "apple" => {
            # Get CPU brand string for Apple Silicon identification
            sysctl -n machdep.cpu.brand_string | str trim
        }
        _ => "cpu"
    }
}

# Get the ONNX Runtime execution provider for this GPU type
def get_onnx_ep [gpu_type: string]: nothing -> string {
    match $gpu_type {
        "amd" => "ROCMExecutionProvider"
        "nvidia" => "CUDAExecutionProvider"
        "apple" => "CoreMLExecutionProvider"
        _ => "CPUExecutionProvider"
    }
}

# Get the Kubernetes device plugin manifest URL
def get_device_plugin [gpu_type: string]: nothing -> string {
    match $gpu_type {
        "amd" => "https://raw.githubusercontent.com/ROCm/k8s-device-plugin/master/k8s-ds-amdgpu-dp.yaml"
        "nvidia" => "nvidia-device-plugin"  # Typically installed via GPU operator
        _ => ""
    }
}

# Get the Kubernetes resource name for GPU requests
def get_k8s_resource [gpu_type: string]: nothing -> string {
    match $gpu_type {
        "amd" => "amd.com/gpu"
        "nvidia" => "nvidia.com/gpu"
        _ => ""
    }
}

# Get the appropriate Docker base image for this GPU type
def get_docker_base [gpu_type: string]: nothing -> string {
    match $gpu_type {
        "amd" => "rocm/dev-ubuntu-22.04:6.0"
        "nvidia" => "nvidia/cuda:12.2-runtime-ubuntu22.04"
        _ => "debian:bookworm-slim"
    }
}

# Get environment variables needed for this GPU type and architecture
def get_env_vars [gpu_type: string, arch: string]: nothing -> record {
    let base = { ONNX_EXECUTION_PROVIDER: (get_onnx_ep $gpu_type) }

    match $gpu_type {
        "amd" => {
            # RDNA 3 (gfx1102) and similar need version override for ROCm compatibility
            if ($arch | str contains "gfx1102") or ($arch | str contains "gfx1101") or ($arch | str contains "gfx1100") {
                $base | merge { HSA_OVERRIDE_GFX_VERSION: "11.0.0" }
            } else if ($arch | str contains "gfx1030") or ($arch | str contains "gfx1031") {
                # RDNA 2
                $base | merge { HSA_OVERRIDE_GFX_VERSION: "10.3.0" }
            } else {
                $base
            }
        }
        "nvidia" => {
            # NVIDIA typically doesn't need extra env vars for ONNX Runtime
            $base
        }
        "apple" => {
            # Apple CoreML provider doesn't need extra env vars
            $base
        }
        _ => $base
    }
}

# Ensure the Kubernetes GPU device plugin is installed
export def ensure-plugin [gpu: record] {
    if ($gpu.device_plugin | is-empty) {
        print "No GPU device plugin needed for this configuration"
        return
    }

    if $gpu.type == "amd" {
        print $"Installing AMD GPU device plugin from ($gpu.device_plugin)"
        kubectl apply -f $gpu.device_plugin
        kubectl rollout status daemonset/amdgpu-device-plugin-daemonset -n kube-system --timeout=60s
        print "AMD GPU device plugin installed successfully"
    } else if $gpu.type == "nvidia" {
        print "NVIDIA GPU support typically uses the GPU Operator."
        print "Install via: helm install gpu-operator nvidia/gpu-operator"
    }
}

# Check if the system has GPU support available
export def is-available []: nothing -> bool {
    let gpu = (detect)
    $gpu.type != "none"
}

# Print a summary of detected GPU configuration
export def info [] {
    let gpu = (detect)

    print $"GPU Type: ($gpu.type)"
    print $"Architecture: ($gpu.arch)"
    print $"ONNX Execution Provider: ($gpu.onnx_ep)"

    if ($gpu.k8s_resource | is-not-empty) {
        print $"Kubernetes Resource: ($gpu.k8s_resource)"
    }

    print "Environment Variables:"
    for kv in ($gpu.env_vars | transpose key value) {
        print $"  ($kv.key)=($kv.value)"
    }
}
