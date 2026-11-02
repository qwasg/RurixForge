/** HELIX07 SceneContract camera interpolation; shared by the native sampler and CPU tests. */
export function lookRotation(eye, target) {
  const norm = (v) => { const n = Math.hypot(...v); if (!(n > 0)) throw new Error('Degenerate camera basis'); return v.map((x) => x / n); };
  const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
  const z = norm(eye.map((v, i) => v - target[i]));
  const x = norm(cross(Math.abs(z[1]) > .999 ? [1, 0, 0] : [0, 1, 0], z));
  const y = cross(z, x);
  const m = [0, 1, 2].map((r) => [x[r], y[r], z[r]]);
  const trace = m[0][0] + m[1][1] + m[2][2];
  if (trace > 0) { const s = Math.sqrt(trace + 1) * 2; return [(m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s, s / 4]; }
  const i = m[0][0] > m[1][1] ? (m[0][0] > m[2][2] ? 0 : 2) : (m[1][1] > m[2][2] ? 1 : 2);
  const j = (i + 1) % 3, k = (i + 2) % 3, s = Math.sqrt(1 + m[i][i] - m[j][j] - m[k][k]) * 2;
  const q = [0, 0, 0, 0]; q[i] = s / 4; q[j] = (m[j][i] + m[i][j]) / s; q[k] = (m[k][i] + m[i][k]) / s; q[3] = (m[k][j] - m[j][k]) / s;
  return q;
}

export function trajectoryPose(contract, seconds) {
  if (contract.schema !== 'helix07.trajectory.v1' || contract.frameGeneration !== false || !(contract.durationSeconds > 0)) throw new Error('Unsupported SceneContract');
  const t = ((seconds % contract.durationSeconds) + contract.durationSeconds) % contract.durationSeconds;
  const frames = contract.keyframes;
  let pose = frames.at(-1);
  for (let i = 0; i + 1 < frames.length; i++) {
    const a = frames[i], b = frames[i + 1];
    if (t > b.t) continue;
    let blend = Math.max(0, Math.min(1, (t - a.t) / (b.t - a.t))); blend *= blend * (3 - 2 * blend);
    const mix = (key) => a[key].map((v, k) => v * (1 - blend) + b[key][k] * blend);
    pose = { eye: mix('eye'), target: mix('target'), fov: a.fov * (1 - blend) + b.fov * blend }; break;
  }
  const spot = [-1.3 + Math.sin(t * .7) * 2.1, 3.7, -.3];
  return { t, cycle: Math.floor(seconds / contract.durationSeconds), updates: [
    { id: contract.cameraEntity, translation: pose.eye, rotation: lookRotation(pose.eye, pose.target), cameraFov: pose.fov },
    { id: contract.witnesses.spot, translation: spot, rotation: lookRotation(spot, [Math.sin(t * .5), 0, -4]) },
  ] };
}

export const nvmlQueryFields = ['timestamp', 'uuid', 'name', 'pci.bus_id', 'driver_version', 'memory.used', 'memory.total', 'clocks.current.graphics', 'clocks.current.memory', 'temperature.gpu', 'power.draw'];
export function parseNvmlCsv(line) {
  const cells = []; let cell = '', quoted = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    if (ch === '"') { if (quoted && line[i + 1] === '"') { cell += '"'; i++; } else quoted = !quoted; }
    else if (ch === ',' && !quoted) { cells.push(cell.trim()); cell = ''; }
    else cell += ch;
  }
  cells.push(cell.trim());
  if (quoted || cells.length !== nvmlQueryFields.length || !cells[1].startsWith('GPU-')) throw new Error('Unexpected NVML CSV row');
  const number = (index) => cells[index] !== '' && Number.isFinite(Number(cells[index])) ? Number(cells[index]) : null;
  return { sourceTimestamp: cells[0], uuid: cells[1], name: cells[2], pciBusId: cells[3], driverVersion: cells[4],
    memoryUsedMiB: number(5), memoryTotalMiB: number(6), graphicsClockMHz: number(7), memoryClockMHz: number(8), temperatureC: number(9), powerW: number(10) };
}
