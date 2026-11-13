#!/usr/bin/env nu
# Download pre-converted ONNX models from HuggingFace (run once per machine)
# Stores in ~/.cache/dapple-onnx/
#
# Usage:
#   nu convert_onnx_models.nu              # Download all models
#   nu convert_onnx_models.nu --model embedding  # Download specific model
#   nu convert_onnx_models.nu --force      # Re-download even if exists

# HuggingFace download URL pattern: https://huggingface.co/{org}/{model}/resolve/main/{path}
const HF_BASE = "https://huggingface.co"

# Models with pre-converted ONNX on HuggingFace
const DOWNLOAD_MODELS = [
    {
        name: "embedding"
        hf_repo: "lightonai/modernbert-embed-large"
        dir: "modernbert-embed-large"
        files: ["onnx/model.onnx", "tokenizer.json"]
    }
    {
        name: "mlm"
        hf_repo: "answerdotai/ModernBERT-base"
        dir: "ModernBERT-base"
        files: ["onnx/model.onnx", "tokenizer.json"]
    }
]

def main [
    --model: string = "all"  # "all" | "embedding" | "mlm" | "summarization" | "tts"
    --force                  # Re-download even if exists
] {
    # Use DAPPLE_ONNX_CACHE_DIR if set (from dapple env), otherwise default to ~/.cache/dapple-onnx
    let cache_dir = ($env | get -o DAPPLE_ONNX_CACHE_DIR | default ($env.HOME | path join ".cache" "dapple-onnx"))
    mkdir $cache_dir

    # TTS uses pre-converted VITS models (download only, no conversion)
    if $model == "all" or $model == "tts" {
        download_vits $cache_dir "vits-ljspeech" $force
    }

    # Summarization (T5) requires Python conversion — no pre-converted ONNX available
    if $model == "all" or $model == "summarization" {
        let t5_dir = ($cache_dir | path join "flan-t5-base-samsum")
        let t5_model = ($t5_dir | path join "encoder_model.onnx")
        if not ($t5_model | path exists) {
            print "summarization (T5): no pre-converted ONNX available on HuggingFace"
            print "  T5 is an encoder-decoder model requiring encoder_model.onnx + decoder_model.onnx"
            print "  To convert manually:"
            print "    pip install optimum[onnxruntime] transformers torch"
            print "    python scripts/onnx_convert/convert_t5.py --model-id philschmid/flan-t5-base-samsum --output-dir ~/.cache/dapple-onnx/flan-t5-base-samsum"
        } else {
            print "summarization: already converted, skipping"
        }
    }

    # Download pre-converted ONNX models from HuggingFace
    let to_download = if $model == "all" {
        $DOWNLOAD_MODELS
    } else if $model == "tts" or $model == "summarization" {
        []  # Handled above
    } else {
        let matched = ($DOWNLOAD_MODELS | where name == $model)
        if ($matched | is-empty) {
            print $"Unknown model type: ($model)"
            print "Available: embedding, mlm, summarization, tts, all"
            exit 1
        }
        $matched
    }

    for m in $to_download {
        download_hf_model $cache_dir $m $force
    }

    print ""
    print $"ONNX models ready at ($cache_dir)"
    print ""
    if ($cache_dir | path exists) and ((ls $cache_dir | length) > 0) {
        print "Directory contents:"
        ls $cache_dir | select name type size
    }
}

# Download pre-converted ONNX model files from HuggingFace
def download_hf_model [cache_dir: path, model: record, force: bool] {
    let out_path = ($cache_dir | path join $model.dir)
    let model_file = ($out_path | path join "model.onnx")

    if ($model_file | path exists) and (not $force) {
        print $"($model.name): already downloaded at ($out_path), skipping"
        print $"  Use --force to re-download"
        return
    }

    print $"($model.name): downloading pre-converted ONNX from ($model.hf_repo)..."
    mkdir $out_path

    for file in $model.files {
        let url = $"($HF_BASE)/($model.hf_repo)/resolve/main/($file)"
        # Flatten onnx/model.onnx -> model.onnx in the output directory
        let filename = ($file | path basename)
        let dest = ($out_path | path join $filename)

        print $"  downloading ($file)..."
        try {
            http get --raw $url | save -f $dest
            print $"  saved ($filename) (size: (ls $dest | get 0.size))"
        } catch {|e|
            print $"  ERROR: failed to download ($file): ($e.msg)"
            print $"  URL: ($url)"
        }
    }

    if ($model_file | path exists) {
        print $"($model.name): saved to ($out_path)"
    } else {
        print $"($model.name): ERROR — model.onnx not found after download"
    }
}

# Download a pre-converted VITS model from sherpa-onnx releases
def download_vits [cache_dir: path, model_name: string, force: bool] {
    let vits_models = {
        "vits-ljspeech": {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-ljs.tar.bz2"
            inner_dir: "vits-ljs"
            sample_rate: 22050
            languages: ["en"]
            speakers: 1
        }
        "vits-vctk": {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-vctk.tar.bz2"
            inner_dir: "vits-vctk"
            sample_rate: 22050
            languages: ["en"]
            speakers: 109
        }
        "vits-melo-zh-en": {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/vits-melo-tts-zh_en.tar.bz2"
            inner_dir: "vits-melo-tts-zh_en"
            sample_rate: 44100
            languages: ["zh", "en"]
            speakers: 1
        }
    }

    let model_info = ($vits_models | get $model_name)
    let out_path = ($cache_dir | path join $model_name)
    let model_file = ($out_path | path join "model.onnx")

    if ($model_file | path exists) and (not $force) {
        print $"tts: already downloaded at ($out_path), skipping"
        print $"  Use --force to re-download"
        return
    }

    print $"tts: downloading VITS model ($model_name)..."
    mkdir $out_path

    # Download and extract
    let tmp_dir = (mktemp -d)
    let archive_path = ($tmp_dir | path join "model.tar.bz2")

    print $"  Downloading from ($model_info.url)..."
    http get $model_info.url | save $archive_path

    print "  Extracting..."
    tar -xjf $archive_path -C $tmp_dir

    # Copy files from inner directory.
    #
    # `path join "*"` yields a STRING, and nushell only expands real glob
    # literals — passing it to `cp` looks for a file literally named "*"
    # and fails with a misleading "'<dir>/*' does not exist" even when the
    # directory is present. Expand explicitly with `glob`, and fail loudly
    # (naming what the archive actually contained) if the layout changed.
    let inner_path = ($tmp_dir | path join $model_info.inner_dir)
    let entries = (glob ($inner_path | path join "*"))
    if ($entries | is-empty) {
        let found = (ls $tmp_dir | where type == dir | get name | each { path basename })
        error make {
            msg: $"archive did not contain ($model_info.inner_dir)/ — top-level dirs found: ($found | str join ', ')"
        }
    }
    cp -r ...$entries $out_path

    # Write config.json if not present
    let config_path = ($out_path | path join "config.json")
    if not ($config_path | path exists) {
        let config = {
            audio: { sample_rate: $model_info.sample_rate }
            languages: $model_info.languages
            num_speakers: $model_info.speakers
        }
        $config | to json | save $config_path
    }

    # Cleanup
    rm -rf $tmp_dir

    print $"tts: saved to ($out_path)"
}

# Verify downloaded models exist and are valid
def "main verify" [] {
    let cache_dir = ($env | get -o DAPPLE_ONNX_CACHE_DIR | default ($env.HOME | path join ".cache" "dapple-onnx"))

    if not ($cache_dir | path exists) {
        print "No ONNX models found. Run 'nu convert_onnx_models.nu' first."
        exit 1
    }

    let models = [
        { name: "embedding", dir: "modernbert-embed-large", needs_tokenizer: true }
        { name: "mlm", dir: "ModernBERT-base", needs_tokenizer: true }
        { name: "summarization", dir: "flan-t5-base-samsum", needs_tokenizer: true }
        { name: "tts", dir: "vits-ljspeech", needs_tokenizer: false }  # VITS uses tokens.txt
    ]

    print "Checking ONNX models..."
    print ""

    for m in $models {
        let model_dir = ($cache_dir | path join $m.dir)
        let model_file = ($model_dir | path join "model.onnx")
        let tokenizer_file = if $m.needs_tokenizer {
            ($model_dir | path join "tokenizer.json")
        } else {
            ($model_dir | path join "tokens.txt")
        }

        let has_model = ($model_file | path exists)
        let has_tokenizer = ($tokenizer_file | path exists)

        let status = if $has_model and $has_tokenizer {
            "OK"
        } else if $has_model {
            if $m.needs_tokenizer { "MISSING TOKENIZER" } else { "MISSING TOKENS" }
        } else {
            "NOT DOWNLOADED"
        }

        let size = if $has_model {
            (ls $model_file | get 0.size)
        } else {
            "N/A"
        }

        print $"($m.name | fill -w 15) ($status | fill -w 20) ($size)"
    }
}

# Clean all downloaded models
def "main clean" [] {
    let cache_dir = ($env | get -o DAPPLE_ONNX_CACHE_DIR | default ($env.HOME | path join ".cache" "dapple-onnx"))

    if ($cache_dir | path exists) {
        print $"Removing ($cache_dir)..."
        rm -rf $cache_dir
        print "Done."
    } else {
        print "No ONNX models to clean."
    }
}

# Download a specific VITS TTS model
# Available models: vits-ljspeech, vits-vctk, vits-melo-zh-en
def "main tts" [
    model: string = "vits-ljspeech"  # vits-ljspeech | vits-vctk | vits-melo-zh-en
    --force                           # Re-download even if exists
] {
    let cache_dir = ($env | get -o DAPPLE_ONNX_CACHE_DIR | default ($env.HOME | path join ".cache" "dapple-onnx"))
    mkdir $cache_dir

    let valid_models = ["vits-ljspeech", "vits-vctk", "vits-melo-zh-en"]
    if not ($model in $valid_models) {
        print $"Unknown VITS model: ($model)"
        print $"Available: ($valid_models | str join ', ')"
        exit 1
    }

    download_vits $cache_dir $model $force

    print ""
    print "To use this model:"
    print $"  export ONNX_MODEL_PATH=($cache_dir)"
    print "  export TTS_ENABLED=true"
    print $"  export TTS_MODEL=($model)"
    if $model == "vits-vctk" {
        print "  export TTS_SPEAKER_ID=0  # 0-108 for different voices"
    }
}

# List available VITS TTS models
def "main tts list" [] {
    print "Available VITS TTS models:"
    print ""
    print "  vits-ljspeech    English single speaker, 22050 Hz (default)"
    print "  vits-vctk        English multi-speaker (109 voices), 22050 Hz"
    print "  vits-melo-zh-en  Multilingual Chinese + English, 44100 Hz"
    print ""
    print "Download with: nu convert_onnx_models.nu tts <model-name>"
}
