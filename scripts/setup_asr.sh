#!/usr/bin/env bash
# 安装 Lyrebird 的 ASR 环境（MOSS-Transcribe-Diarize，Python 3.12）
# 用法: scripts/setup_asr.sh [venv 目录，默认项目下 .venv-asr]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENV="${1:-$ROOT/.venv-asr}"

if ! command -v uv >/dev/null 2>&1; then
  echo "未找到 uv，先安装: brew install uv  （或见 https://docs.astral.sh/uv/）"
  exit 1
fi

uv venv --python 3.12 "$VENV"
uv pip install --python "$VENV/bin/python" \
  "moss-transcribe-diarize[torch-runtime] @ git+https://github.com/OpenMOSS/MOSS-Transcribe-Diarize.git" \
  --torch-backend=auto

echo
echo "完成。请在 Lyrebird 设置页把「Python 解释器」设为:"
echo "  $VENV/bin/python"
