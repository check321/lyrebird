# Lyrebird

通过 YouTube 播客学英语的 macOS 桌面 App（Apple Silicon）。

订阅/添加播客视频 → 下载并转写字幕 → AI 断句分段、去口水词、翻译 → 在播放器里边看边点词查词典、攒生词卡 → 复习。

## 功能

- **视频管理**：粘贴 YouTube 链接下载（yt-dlp，支持 cookies 认证），媒体库分类管理，断点续播
- **字幕管线**：优先使用视频自带字幕；无字幕时用本地 ASR（MOSS-Transcribe-Diarize）转写
- **AI 整理精校**：词流级断句 → 本地去口水词（思考声 um/uh、口吃碎片、连续重复、插入语 you know/I mean 等）→ LLM 段落分组；支持一键重新整理
- **AI 翻译**：分块渐进式翻译，句级中文按时间重叠比例在共享 cue 间互补划分（不重复、不遗漏）
- **播放体验**：libass 内嵌字幕渲染（CapCut 式样式预设，就地调节实时生效）、字幕与播放轴双向绑定、沉浸模式暂停时渐显阅读区
- **词典**：本地 ECDICT 秒查 + 按需 AI 上下文解析，支持导入 MDX 词典；发音走在线真人录音，系统英文 TTS 离线兜底
- **生词卡**：点词即查、一键收藏，复习页按遗忘曲线回顾

## 技术栈

- **壳/UI**：Tauri 2 + Svelte 5（SvelteKit，SPA 模式）+ TypeScript + Tailwind CSS 4
- **后端**：Rust（Tauri commands + tokio），SQLite（sqlx，启动时自动迁移）
- **字幕渲染**：libass-wasm（JavascriptSubtitlesOctopus）
- **ASR**：Python sidecar 运行 [MOSS-Transcribe-Diarize](https://github.com/OpenMOSS/MOSS-Transcribe-Diarize)
- **LLM**：OpenAI 兼容协议（默认 DeepSeek），在设置页配置

## 运行环境

- macOS（Apple Silicon）
- Node.js 18+ / npm
- Rust 工具链（rustup）
- [uv](https://docs.astral.sh/uv/)（安装 ASR Python 环境用）
- yt-dlp（视频下载，`brew install yt-dlp`）

## 快速开始

```bash
# 1. 安装前端依赖
npm install

# 2. 安装 ASR Python 环境（.venv-asr/）
scripts/setup_asr.sh

# 3. 开发模式运行
npm run tauri dev
```

首次运行后，到**设置页**完成配置：

- **LLM**：API Key / Base URL / 模型名（OpenAI 兼容协议，默认 DeepSeek）
- **ASR**：Python 解释器路径（setup 脚本末尾会打印）、MOSS-Transcribe-Diarize 模型权重目录（需自行下载，建议放 `models/` 下，该目录已被 gitignore）
- **词典**：ECDICT `stardict.sqlite` 路径（可选导入 MDX 词典）
- **下载**：需要登录态的视频在设置页配置 cookies

## 常用命令

| 命令 | 说明 |
| --- | --- |
| `npm run tauri dev` | 开发模式运行 |
| `npm run check` | 前端类型检查（svelte-check） |
| `cd src-tauri && cargo check` | 后端编译检查 |
| `cd src-tauri && cargo test` | 后端单元测试 |
| `npm run tauri build` | 打包发布 |
| `scripts/setup_asr.sh` | 安装/重装 ASR Python 环境 |

## 项目结构

```
src/                Svelte 前端（路由、阅读区、字幕样式面板等）
src/lib/            ass.ts（cue→ASS）、speech.ts（查词发音）等共享模块
src-tauri/src/      Rust 后端：ingest（下载）、asr、subtitle、translate、words、dict、llm、media（本地流媒体服务器）
src-tauri/migrations/  SQLite 迁移（启动时自动执行）
python/asr_worker.py   ASR sidecar
static/fonts/       字幕字体（页面 @font-face 与 libass 共用）
```

## 数据与隐私

- 所有数据（视频库、字幕、生词卡、设置）存于本地 SQLite；API Key 等敏感配置保存在本地数据库，不上传任何服务器
- 仓库不提交：`models/`（模型权重）、`.venv-asr/`、`build/`、`.env*` 等（见 `.gitignore`）

## License

Apache-2.0（见 [LICENSE](LICENSE)）
