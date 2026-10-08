export type CommandSettings = {
  volume: number; overlay: number; reducedMotion: boolean; showTutorial: boolean; showNotices: boolean; shortcuts: boolean;
};
export const DEFAULT_COMMAND_SETTINGS: CommandSettings = {
  volume: .24, overlay: 1, reducedMotion: false, showTutorial: true, showNotices: true, shortcuts: true,
};
const STORAGE = 'code-sentinels-preferences-v1';
export function loadCommandSettings(): CommandSettings {
  try {
    const value = JSON.parse(localStorage.getItem(STORAGE) || '{}');
    return { volume: typeof value.volume === 'number' && Number.isFinite(value.volume) ? Math.max(0, Math.min(1, value.volume)) : DEFAULT_COMMAND_SETTINGS.volume,
      overlay: [0, 1, 2].includes(value.overlay) ? value.overlay : DEFAULT_COMMAND_SETTINGS.overlay,
      ...Object.fromEntries(['reducedMotion', 'showTutorial', 'showNotices', 'shortcuts'].map(key => [key,
        typeof value[key] === 'boolean' ? value[key] : DEFAULT_COMMAND_SETTINGS[key as keyof CommandSettings]])) } as CommandSettings;
  } catch { return { ...DEFAULT_COMMAND_SETTINGS }; }
}
export function saveCommandSettings(settings: CommandSettings) {
  try { localStorage.setItem(STORAGE, JSON.stringify(settings)); } catch { /* Preferences still apply in this session. */ }
}
let audio: AudioContext | null = null;
/** A short interface confirmation tone, separate from native combat audio. */
export function playCommandTone(volume: number) {
  if (volume <= 0 || typeof AudioContext === 'undefined') return;
  try {
    audio ??= new AudioContext();
    if (audio.state === 'suspended') void audio.resume().catch(() => {});
    const oscillator = audio.createOscillator(), gain = audio.createGain(), time = audio.currentTime;
    oscillator.type = 'sine'; oscillator.frequency.setValueAtTime(680, time); oscillator.frequency.exponentialRampToValueAtTime(430, time + .055);
    gain.gain.setValueAtTime(Math.max(.0001, volume * .065), time); gain.gain.exponentialRampToValueAtTime(.0001, time + .065);
    oscillator.connect(gain); gain.connect(audio.destination); oscillator.start(time); oscillator.stop(time + .075);
    oscillator.onended = () => { oscillator.disconnect(); gain.disconnect(); };
  } catch { /* An unavailable audio device must never prevent a command. */ }
}
