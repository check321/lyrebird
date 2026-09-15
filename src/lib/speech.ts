import { invoke } from "@tauri-apps/api/core";

let current: HTMLAudioElement | null = null;

/** 查词发音：有道真人录音（在线，type=2 美音），失败回退 Rust 侧系统 TTS */
export function speakWord(word: string) {
  const w = word.trim();
  if (!w) return;

  current?.pause();
  const el = new Audio(
    `https://dict.youdao.com/dictvoice?audio=${encodeURIComponent(w)}&type=2`,
  );
  current = el;

  let settled = false;
  const fallback = () => {
    if (settled) return;
    settled = true;
    el.pause();
    invoke("speak_word", { word: w }).catch(() => {});
  };

  // 网络挂起而非报错时，限时回退
  const timer = setTimeout(() => {
    if (el.readyState < HTMLMediaElement.HAVE_CURRENT_DATA) fallback();
  }, 4000);
  el.addEventListener("playing", () => {
    settled = true;
    clearTimeout(timer);
  });
  el.addEventListener("error", () => {
    clearTimeout(timer);
    fallback();
  });
  el.play().catch(() => {
    clearTimeout(timer);
    fallback();
  });
}
