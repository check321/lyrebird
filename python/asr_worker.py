#!/usr/bin/env python3
"""Lyrebird ASR worker: 用 MOSS-Transcribe-Diarize 转写音频，JSON 分段输出到 stdout。

用法: python asr_worker.py --model <模型目录> --audio <音频文件> [--max-new-tokens 65536]

需要 Python 3.12+ 环境并安装 moss-transcribe-diarize 包：
    uv venv --python 3.12 .venv
    uv pip install "git+https://github.com/OpenMOSS/MOSS-Transcribe-Diarize[torch-runtime]"
"""
import argparse
import json
import os
import sys


def ensure_remote_code(model_dir: str) -> None:
    """模型快照缺少 trust_remote_code 的 .py 文件时，从已安装的包里补齐。"""
    import shutil
    import moss_transcribe_diarize

    pkg_dir = os.path.dirname(moss_transcribe_diarize.__file__)
    for name in (
        "configuration_moss_transcribe_diarize.py",
        "modeling_moss_transcribe_diarize.py",
        "processing_moss_transcribe_diarize.py",
    ):
        dst = os.path.join(model_dir, name)
        src = os.path.join(pkg_dir, name)
        if not os.path.exists(dst) and os.path.exists(src):
            shutil.copy(src, dst)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True, help="模型权重目录（本地路径）")
    ap.add_argument("--audio", required=True, help="输入音频（建议 16kHz WAV）")
    ap.add_argument("--max-new-tokens", type=int, default=65536)
    args = ap.parse_args()

    try:
        import torch
        from transformers import AutoModelForCausalLM, AutoProcessor
        from moss_transcribe_diarize import parse_transcript
        from moss_transcribe_diarize.inference_utils import (
            build_transcription_messages,
            generate_transcription,
            resolve_device,
        )
    except ImportError as e:
        print(
            json.dumps({"error": f"缺少依赖: {e}。请先运行 scripts/setup_asr.sh 安装 ASR 环境"}),
            file=sys.stdout,
        )
        return 2

    device = resolve_device("auto")
    dtype = torch.bfloat16 if device.type == "cuda" else torch.float32

    ensure_remote_code(args.model)

    model = (
        AutoModelForCausalLM.from_pretrained(
            args.model,
            trust_remote_code=True,
            dtype="auto",
            attn_implementation="sdpa",
        )
        .to(dtype=dtype)
        .to(device)
        .eval()
    )
    processor = AutoProcessor.from_pretrained(args.model, trust_remote_code=True)

    messages = build_transcription_messages(args.audio)
    result = generate_transcription(
        model,
        processor,
        messages,
        max_new_tokens=args.max_new_tokens,
        do_sample=False,
        device=device,
        dtype=dtype,
    )

    segments = [
        {
            "start": round(seg.start, 3),
            "end": round(seg.end, 3),
            "speaker": seg.speaker,
            "text": seg.text.strip(),
        }
        for seg in parse_transcript(result["text"])
        if seg.text.strip()
    ]
    print(json.dumps({"segments": segments}, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
