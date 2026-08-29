import { useEffect, useRef, useState } from 'react';

/**
 * Composer 语音输入(听写)数据面:封装浏览器原生 Web Speech API
 * (SpeechRecognition / webkitSpeechRecognition),把识别文本实时喂回 Composer 草稿——
 * 未定稿片段边说边刷,定稿后落定,与手打文本共用同一个 textarea,不另开输入通道。
 *
 * 能力边界如实留痕:该 API 的识别服务是 Chrome/Edge 官方构建自带的私有服务,
 * 普通 Chromium(含 apps/desktop 的 Electron 壳)不含其密钥,start() 后必报
 * error=network(electron/electron#31732、#46143,且 GOOGLE_API_KEY 对该 API 无效)。
 * 故本模块只做「可用即用、不可用如实说」:构造器缺失 → supported=false 按钮禁用;
 * 识别服务不可达 → 原样弹 toast 说明该走 Chrome/Edge,绝不静默吞错或伪造识别结果。
 * 若后续要覆盖桌面壳,须另起后端转写腿(录音 → ASR 服务),不在本模块口径内。
 */

/** 默认识别语言(界面为简体中文;中英混说由 zh-CN 识别器兜底)。 */
export const DEFAULT_SPEECH_LANG = 'zh-CN';

/** 3 秒内自动重开超过此次数即判为反复中断,停听报错(防静默失败下的重启风暴)。 */
const RESTART_BURST_LIMIT = 8;
const RESTART_WINDOW_MS = 3000;

// ---- Web Speech API 最小类型面(TS DOM lib 未收录,按 W3C 草案取用到的成员) ----

interface SpeechAlternativeLike {
  readonly transcript: string;
}

interface SpeechResultLike {
  readonly isFinal: boolean;
  readonly length: number;
  readonly [index: number]: SpeechAlternativeLike;
}

interface SpeechResultListLike {
  readonly length: number;
  readonly [index: number]: SpeechResultLike;
}

interface SpeechResultEventLike {
  readonly results: SpeechResultListLike;
}

interface SpeechErrorEventLike {
  readonly error: string;
}

interface SpeechRecognitionLike {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  maxAlternatives: number;
  start(): void;
  stop(): void;
  abort(): void;
  onresult: ((e: SpeechResultEventLike) => void) | null;
  onerror: ((e: SpeechErrorEventLike) => void) | null;
  onend: (() => void) | null;
}

type SpeechRecognitionCtor = new () => SpeechRecognitionLike;

function speechCtor(): SpeechRecognitionCtor | null {
  const w = globalThis as Record<string, unknown>;
  const ctor = w.SpeechRecognition ?? w.webkitSpeechRecognition;
  return typeof ctor === 'function' ? (ctor as SpeechRecognitionCtor) : null;
}

export function speechSupported(): boolean {
  return speechCtor() !== null;
}

/** 致命错误码 → 面向用户的说明;未列出的按非致命处理(no-speech/aborted 静音重开)。 */
const FATAL_ERROR_TEXT: Record<string, string> = {
  'not-allowed': '麦克风权限被拒绝，请在浏览器站点设置里放行后重试',
  'service-not-allowed': '当前环境不提供语音识别服务，请改用 Chrome / Edge 打开',
  'audio-capture': '未检测到可用麦克风',
  network: '语音识别服务不可达（桌面壳内不含 Chrome 私有识别服务，请改用 Chrome / Edge 打开）',
  'language-not-supported': `识别语言 ${DEFAULT_SPEECH_LANG} 不受支持`,
  'bad-grammar': '语音识别语法错误',
};

/**
 * 底稿与识别文本拼接:仅当两侧都是 ASCII 字母数字时补空格,
 * 中文侧直接相接(识别结果自带标点,再补空格会脏)。
 */
export function joinDictation(base: string, spoken: string): string {
  if (base === '' || spoken === '') return base + spoken;
  const glue = /[A-Za-z0-9]$/.test(base) && /^[A-Za-z0-9]/.test(spoken) ? ' ' : '';
  return `${base}${glue}${spoken}`;
}

/** 0 → 0:00;95 → 1:35(按钮上的已录时长)。 */
export function formatElapsed(seconds: number): string {
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${String(s).padStart(2, '0')}`;
}

export interface SpeechInputOptions {
  lang?: string;
  /** 开听时取底稿(当前草稿);识别文本一律接在其后。 */
  onStart: () => string;
  /** 识别推进:底稿 + 已识别文本(含未定稿片段)的完整值。 */
  onText: (next: string) => void;
  /** 致命错误(报错即已停听)。 */
  onError: (message: string) => void;
}

export interface SpeechInput {
  /** 本环境是否有 SpeechRecognition 构造器(不代表识别服务可达)。 */
  supported: boolean;
  listening: boolean;
  /** 已听写秒数。 */
  seconds: number;
  toggle: () => void;
  stop: () => void;
  /** 听写途中用户手打改了输入框:以新值为底稿重开一段,已注入部分不再重复。 */
  rebase: (base: string) => void;
}

export function useSpeechInput(options: SpeechInputOptions): SpeechInput {
  // 调用方每次渲染都传新闭包,识别回调里一律读最新的一份
  const optsRef = useRef(options);
  optsRef.current = options;

  const [supported] = useState(speechSupported);
  const [listening, setListening] = useState(false);
  const [seconds, setSeconds] = useState(0);

  const recRef = useRef<SpeechRecognitionLike | null>(null);
  /** 用户意图仍在听(区别于识别器静音自停);置 false 后到达的结果一律丢弃。 */
  const wantRef = useRef(false);
  const baseRef = useRef('');
  /** 跨识别器重开累计的定稿文本。 */
  const carryRef = useRef('');
  /** 当前识别器内已定稿文本(重开时并入 carry)。 */
  const segFinalRef = useRef('');
  /** 当前识别器 results 的起算下标(rebase 时前移到已收到的条数)。 */
  const offsetRef = useRef(0);
  const resultCountRef = useRef(0);
  const restartsRef = useRef<number[]>([]);

  const finish = () => {
    wantRef.current = false;
    recRef.current = null;
    restartsRef.current = [];
    setListening(false);
    setSeconds(0);
  };

  const start = () => {
    const Ctor = speechCtor();
    if (Ctor === null || wantRef.current) return;

    const rec = new Ctor();
    rec.lang = optsRef.current.lang ?? DEFAULT_SPEECH_LANG;
    rec.continuous = true;
    rec.interimResults = true;
    rec.maxAlternatives = 1;

    rec.onresult = (e) => {
      if (!wantRef.current) return;
      const results = e.results;
      resultCountRef.current = results.length;
      let fin = '';
      let interim = '';
      for (let i = offsetRef.current; i < results.length; i += 1) {
        const r = results[i];
        const t = r[0]?.transcript ?? '';
        if (r.isFinal) fin += t;
        else interim += t;
      }
      segFinalRef.current = fin;
      optsRef.current.onText(joinDictation(baseRef.current, carryRef.current + fin + interim));
    };

    rec.onerror = (e) => {
      const text = FATAL_ERROR_TEXT[e.error];
      // no-speech / aborted 非致命:交给 onend 的静音重开腿
      if (text === undefined) return;
      finish();
      optsRef.current.onError(text);
    };

    rec.onend = () => {
      // 连续听写下识别器仍会在长静音后自停:用户没喊停就并入定稿重开
      if (!wantRef.current) {
        finish();
        return;
      }
      const now = Date.now();
      restartsRef.current = restartsRef.current.filter((t) => now - t < RESTART_WINDOW_MS);
      restartsRef.current.push(now);
      if (restartsRef.current.length > RESTART_BURST_LIMIT) {
        finish();
        optsRef.current.onError('语音识别反复中断，已停止听写');
        return;
      }
      carryRef.current += segFinalRef.current;
      segFinalRef.current = '';
      offsetRef.current = 0;
      resultCountRef.current = 0;
      try {
        rec.start();
      } catch {
        finish();
      }
    };

    recRef.current = rec;
    wantRef.current = true;
    baseRef.current = optsRef.current.onStart();
    carryRef.current = '';
    segFinalRef.current = '';
    offsetRef.current = 0;
    resultCountRef.current = 0;
    restartsRef.current = [];
    try {
      rec.start();
    } catch (err) {
      finish();
      optsRef.current.onError(`语音识别启动失败：${err instanceof Error ? err.message : String(err)}`);
      return;
    }
    setListening(true);
    setSeconds(0);
  };

  const stop = () => {
    const rec = recRef.current;
    // 先落 wantRef:stop() 后识别器仍可能补投一次定稿结果,此时草稿已由调用方接管
    finish();
    try {
      rec?.stop();
    } catch {
      // 已停/未起,幂等
    }
  };

  const toggle = () => {
    if (wantRef.current) stop();
    else start();
  };

  const rebase = (base: string) => {
    if (!wantRef.current) return;
    baseRef.current = base;
    carryRef.current = '';
    segFinalRef.current = '';
    offsetRef.current = resultCountRef.current;
  };

  useEffect(() => {
    if (!listening) return;
    const id = setInterval(() => setSeconds((s) => s + 1), 1000);
    return () => clearInterval(id);
  }, [listening]);

  useEffect(
    () => () => {
      wantRef.current = false;
      try {
        recRef.current?.abort();
      } catch {
        // 卸载时识别器可能已被回收
      }
      recRef.current = null;
    },
    [],
  );

  return { supported, listening, seconds, toggle, stop, rebase };
}
