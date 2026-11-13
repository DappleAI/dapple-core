# TTS Service

> **Audience:** Backend Developers · Platform Engineers · Journey Authors

The conduit provides text-to-speech synthesis via VITS (Variational Inference with adversarial learning for end-to-end Text-to-Speech), a lightweight neural TTS model that works efficiently on CPU across all platforms including Mac laptops.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      TTS Service                                │
├─────────────────────────────────────────────────────────────────┤
│  SynthesizeRequest                                              │
│  ├─ text: String          (what to say)                         │
│  └─ voice: Option<String> (unused, kept for API compat)         │
│                                                                 │
│  TtsBackend trait                                               │
│  ├─ VitsTtsBackend (neural TTS via ONNX Runtime)                │
│  └─ StubTtsBackend (testing)                                    │
│                                                                 │
│  SynthesizeResult                                               │
│  ├─ audio_bytes: Vec<u8>   (WAV format)                         │
│  ├─ sample_rate: u32       (22050 Hz for most models)           │
│  └─ duration_seconds: f32                                       │
└─────────────────────────────────────────────────────────────────┘
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `TTS_ENABLED` | `false` | Enable TTS service |
| `TTS_REQUIRED` | `false` | Fail startup if TTS can't load |
| `TTS_MODEL` | `vits-ljspeech` | VITS model name |
| `TTS_SPEAKER_ID` | (none) | Speaker ID for multi-speaker models (0-108 for VCTK) |
| `TTS_LANGUAGE` | (none) | Language validation (e.g., "en", "zh") |
| `TTS_MAX_TEXT_LENGTH` | `500` | Max input characters |
| `ONNX_MODEL_PATH` | `~/.cache/dapple-onnx` | Path to ONNX models |

### Available Models

| Model | Size | Languages | Speakers | Sample Rate |
|-------|------|-----------|----------|-------------|
| `vits-ljspeech` | ~109 MB | English | 1 | 22050 Hz |
| `vits-vctk` | ~116 MB | English | 109 | 22050 Hz |
| `vits-melo-zh-en` | ~163 MB | Chinese + English | 1 | 44100 Hz |

### Minimal Setup

```bash
# Download the default model
nu assets/nu/k8s/convert_onnx_models.nu tts vits-ljspeech

# Run conduit with TTS
TTS_ENABLED=true cargo run -p dapple-prompt-conduit
```

### Multi-Speaker Setup (VCTK)

```bash
# Download multi-speaker model
nu assets/nu/k8s/convert_onnx_models.nu tts vits-vctk

# Run with specific speaker
TTS_ENABLED=true \
TTS_MODEL=vits-vctk \
TTS_SPEAKER_ID=23 \
cargo run -p dapple-prompt-conduit
```

### Multilingual Setup

```bash
# Download Chinese + English model
nu assets/nu/k8s/convert_onnx_models.nu tts vits-melo-zh-en

# Run
TTS_ENABLED=true \
TTS_MODEL=vits-melo-zh-en \
cargo run -p dapple-prompt-conduit
```

## VITS Backend

VITS is a fast, lightweight TTS model that uses ONNX Runtime for inference. Unlike some TTS systems that require voice descriptions, VITS uses:

- **Fixed voices** for single-speaker models (ljspeech)
- **Speaker IDs** for multi-speaker models (VCTK has 109 voices, IDs 0-108)

### Speaker Selection (VCTK)

The VCTK model includes 109 different English speakers with varying accents and characteristics:

```bash
# Speaker 0 - Female, clear
TTS_SPEAKER_ID=0

# Speaker 23 - Male, British
TTS_SPEAKER_ID=23

# Speaker 87 - Female, American
TTS_SPEAKER_ID=87
```

Experiment with different speaker IDs to find voices that suit your needs.

## API Endpoint

### POST /synthesize

Synthesize speech from text.

**Request:**
```json
{
  "text": "Hello, this is a test of the text to speech system."
}
```

**Response:**
```json
{
  "audio_bytes": "UklGRi...",
  "duration_seconds": 3.5,
  "sample_rate": 22050,
  "content_length": 154394,
  "text": "Hello, this is a test of the text to speech system."
}
```

| Field | Type | Description |
|-------|------|-------------|
| `audio_bytes` | string | Base64-encoded WAV audio |
| `duration_seconds` | float | Audio duration |
| `sample_rate` | int | Sample rate (Hz) |
| `content_length` | int | Raw audio size in bytes |
| `text` | string | Original input text |

### Testing the Endpoint

```bash
# Quick test
curl -s -X POST http://localhost:8081/synthesize \
  -H "Content-Type: application/json" \
  -d '{"text": "Hello world"}' | jq '.duration_seconds'

# Save audio file
curl -s -X POST http://localhost:8081/synthesize \
  -H "Content-Type: application/json" \
  -d '{"text": "Hello world"}' \
  | jq -r '.audio_bytes' | base64 -d > output.wav

# Play audio (Linux)
aplay output.wav

# Play audio (macOS)
afplay output.wav
```

## Nushell Integration

The `synth` command in nushell wraps the TTS endpoint:

```nushell
# Basic synthesis
"Hello world" | synth

# Save to file
"Hello world" | synth | save audio.wav
```

## Local Development

### Downloading Models

Use the nushell script to download models:

```bash
# List available models
nu assets/nu/k8s/convert_onnx_models.nu tts list

# Download default English model
nu assets/nu/k8s/convert_onnx_models.nu tts vits-ljspeech

# Download multi-speaker model
nu assets/nu/k8s/convert_onnx_models.nu tts vits-vctk

# Download multilingual model
nu assets/nu/k8s/convert_onnx_models.nu tts vits-melo-zh-en

# Force re-download
nu assets/nu/k8s/convert_onnx_models.nu tts vits-ljspeech --force
```

### GPU Acceleration

VITS uses ONNX Runtime which automatically detects and uses available hardware:

| Platform | Execution Provider | Notes |
|----------|-------------------|-------|
| AMD GPU | ROCm | Auto-detected |
| NVIDIA GPU | CUDA | Auto-detected |
| Apple Silicon | CoreML | Auto-detected |
| CPU | CPU | Fallback, still fast |

Override with `ONNX_EXECUTION_PROVIDER` environment variable if needed:

```bash
# Force CPU
ONNX_EXECUTION_PROVIDER=CPUExecutionProvider \
TTS_ENABLED=true cargo run -p dapple-prompt-conduit
```

**Performance** (VITS is much faster than previous TTS systems):

| Device | Model | Synthesis Time (10 words) |
|--------|-------|---------------------------|
| CPU | vits-ljspeech | ~1-2 seconds |
| GPU | vits-ljspeech | < 1 second |

### Troubleshooting

**Model not found:**
```
VITS ONNX model not found at /home/user/.cache/dapple-onnx/vits-ljspeech
```
→ Download the model: `nu assets/nu/k8s/convert_onnx_models.nu tts vits-ljspeech`

**Language not supported:**
```
Language 'fr' not supported by model vits-ljspeech (supports: en)
```
→ Use a model that supports the required language, or remove `TTS_LANGUAGE`

**Speaker ID out of range:**
```
Speaker ID 200 is out of range for model vits-vctk (max: 108)
```
→ Use a valid speaker ID (0-108 for VCTK)

## Kubernetes Deployment

VITS is lightweight and works well on CPU:

```yaml
# Minimal resources for TTS-enabled conduit
resources:
  requests:
    memory: "512Mi"
    cpu: "500m"
  limits:
    memory: "1Gi"
```

### Model Persistence

Mount a PVC to cache downloaded models:

```yaml
env:
  - name: ONNX_MODEL_PATH
    value: /models
  - name: TTS_ENABLED
    value: "true"
  - name: TTS_MODEL
    value: vits-ljspeech

volumeMounts:
  - name: onnx-models
    mountPath: /models

volumes:
  - name: onnx-models
    persistentVolumeClaim:
      claimName: onnx-models-pvc
```

## Next Steps

- [LLM Backends](./llm-backends.md) — Text generation
- [Embedding Backends](./embedding-backends.md) — Vector embeddings
- [Custom Commands Reference](../nushell/custom-commands.md) — Using `synth` in tasks
