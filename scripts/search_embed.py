"""Inference-only JSON subprocess for examples/semantic_search.plenty.

Setup: python scripts/search_embed.py --setup CACHE
Inference: python scripts/search_embed.py MODEL document|query CACHE
stdin is an array of strings; stdout is an array of 384-dimensional vectors.
"""

import importlib.metadata
import json
from pathlib import Path
import sys

REVISION = "aa8f8b060edb00e03bfdd08813a2949946c8ba55"
MODEL = "bge-small-en-v1.5:" + REVISION + ":fastembed-0.9.0:chars400-overlap64-v1"
REPOSITORY = "Qdrant/bge-small-en-v1.5-onnx-Q"


def main():
    if sys.argv[1:] == ["--identity"]:
        print(MODEL)
        return
    if importlib.metadata.version("fastembed") != "0.9.0":
        raise ValueError("install fastembed==0.9.0")
    from huggingface_hub import snapshot_download

    setup = len(sys.argv) == 3 and sys.argv[1] == "--setup"
    if not setup and (len(sys.argv) != 4 or sys.argv[1] != MODEL
                      or sys.argv[2] not in ("document", "query")):
        raise ValueError(__doc__)
    cache = sys.argv[2] if setup else sys.argv[3]
    path = snapshot_download(
        REPOSITORY, revision=REVISION, cache_dir=cache,
        allow_patterns=["*.json", "model_optimized.onnx"],
        local_files_only=not setup,
    )
    if setup:
        print(f"Model ready: {path}")
        return
    texts = json.load(sys.stdin)
    if not isinstance(texts, list) or not all(isinstance(t, str) for t in texts):
        raise ValueError("expected an array of strings")
    # Detect overflow before FastEmbed's tokenizer silently truncates input.
    from tokenizers import Tokenizer

    tokenizer = Tokenizer.from_file(str(Path(path) / "tokenizer.json"))
    tokenizer.no_truncation()
    tokenizer.no_padding()
    if any(len(tokenizer.encode(t).ids) > 512 for t in texts):
        raise ValueError("input exceeds the model's 512-token limit")
    from fastembed import TextEmbedding

    model = TextEmbedding(
        "BAAI/bge-small-en-v1.5", specific_model_path=path,
        local_files_only=True, providers=["CPUExecutionProvider"], threads=2,
    )
    embed = model.query_embed if sys.argv[2] == "query" else model.passage_embed
    vectors = [v.tolist() for v in embed(texts, batch_size=32)] if texts else []
    json.dump(vectors, sys.stdout, allow_nan=False)
    sys.stdout.write("\n")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"embedding inference: {error}", file=sys.stderr)
        sys.exit(1)
