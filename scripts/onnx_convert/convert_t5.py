#!/usr/bin/env python3
"""
Convert T5 summarization model to ONNX format.

T5 is an encoder-decoder model, so we export both encoder and decoder separately.

Usage:
    python convert_t5.py --model-id philschmid/flan-t5-base-samsum --output-dir ~/.cache/dapple-onnx/flan-t5-base-samsum

Requires:
    pip install optimum[onnxruntime] transformers torch
"""

import argparse
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description="Convert T5 model to ONNX")
    parser.add_argument("--model-id", required=True, help="HuggingFace model ID")
    parser.add_argument("--output-dir", required=True, help="Output directory for ONNX model")
    parser.add_argument("--opset", type=int, default=14, help="ONNX opset version")
    args = parser.parse_args()

    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    print(f"Converting {args.model_id} to ONNX...")

    try:
        from optimum.onnxruntime import ORTModelForSeq2SeqLM
        from transformers import AutoTokenizer

        # Export model to ONNX using Optimum
        # This will create encoder_model.onnx and decoder_model.onnx (or merged versions)
        model = ORTModelForSeq2SeqLM.from_pretrained(
            args.model_id,
            export=True,
        )
        model.save_pretrained(output_dir)

        # Also save tokenizer
        tokenizer = AutoTokenizer.from_pretrained(args.model_id)
        tokenizer.save_pretrained(output_dir)

        print(f"Successfully saved ONNX model to {output_dir}")
        print(f"Files: {list(output_dir.iterdir())}")

    except ImportError as e:
        print(f"Missing dependency: {e}")
        print("Install with: pip install optimum[onnxruntime] transformers torch")
        raise


if __name__ == "__main__":
    main()
