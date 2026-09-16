/** 复习页音效：Web Audio 合成（无素材文件），可全局开关 */

let ctx: AudioContext | null = null;
let enabled = true;

export function setSfxEnabled(v: boolean) {
  enabled = v;
}

function ac(): AudioContext | null {
  if (!enabled) return null;
  ctx ??= new AudioContext();
  if (ctx.state === "suspended") ctx.resume();
  return ctx;
}

function beep(
  freq: number,
  dur: number,
  type: OscillatorType,
  gain = 0.08,
  delay = 0,
) {
  const c = ac();
  if (!c) return;
  const t = c.currentTime + delay;
  const osc = c.createOscillator();
  const g = c.createGain();
  osc.type = type;
  osc.frequency.value = freq;
  g.gain.setValueAtTime(gain, t);
  g.gain.exponentialRampToValueAtTime(0.001, t + dur);
  osc.connect(g).connect(c.destination);
  osc.start(t);
  osc.stop(t + dur);
}

/** 倒计时滴答：短促方波 */
export const sfxTick = () => beep(1050, 0.05, "square", 0.045);
/** 答对：C5→E5 上行双音 */
export const sfxCorrect = () => {
  beep(523.25, 0.12, "sine", 0.09);
  beep(783.99, 0.22, "sine", 0.09, 0.1);
};
/** 答错/超时：低频嗡鸣 */
export const sfxWrong = () => beep(160, 0.28, "sawtooth", 0.06);
