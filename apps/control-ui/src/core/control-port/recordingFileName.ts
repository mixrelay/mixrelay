let sequence = 0;

/** A readable local timestamp plus per-session sequence separates simultaneous recorders. */
export function recordingFileName(now = new Date()): string {
  const pad = (value: number, width = 2) => String(value).padStart(width, '0');
  const day = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}-${pad(now.getMinutes())}-${pad(now.getSeconds())}-${pad(now.getMilliseconds(), 3)}`;
  sequence += 1;
  return `MixRelay-${day}_${time}_${sequence}.wav`;
}
