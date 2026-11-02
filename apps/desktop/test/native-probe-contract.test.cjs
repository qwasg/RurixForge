const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { pathToFileURL } = require('node:url');
const helpers = import(pathToFileURL(path.resolve(__dirname, '../../../scripts/native-probe-contract.mjs')).href);

test('native trajectory uses the project contract IDs and smoothstep waypoints', async () => {
  const { trajectoryPose } = await helpers;
  const contract = JSON.parse(fs.readFileSync(path.resolve(__dirname, '../../../projects/aaa-scene-lab/SceneContract.json')));
  const initial = trajectoryPose(contract, 0);
  const midpoint = trajectoryPose(contract, contract.keyframes[1].t / 2);
  assert.equal(initial.updates[0].id, contract.cameraEntity);
  assert.equal(initial.updates[1].id, contract.witnesses.spot);
  assert.deepEqual(initial.updates[0].translation, contract.keyframes[0].eye);
  assert.deepEqual(midpoint.updates[0].translation, contract.keyframes[0].eye.map((v, i) => (v + contract.keyframes[1].eye[i]) / 2));
  assert.equal(midpoint.updates[0].cameraFov, (contract.keyframes[0].fov + contract.keyframes[1].fov) / 2);
  const loop = trajectoryPose(contract, contract.durationSeconds);
  assert.equal(loop.cycle, 1);
  assert.deepEqual(loop.updates, initial.updates);
});

test('camera look rotation is normalized and keeps the local minus-Z camera convention', async () => {
  const { lookRotation } = await helpers;
  assert.deepEqual(lookRotation([0, 0, 1], [0, 0, 0]), [0, 0, 0, 1]);
  const q = lookRotation([1, 0, 0], [0, 0, 0]);
  assert.ok(Math.abs(Math.hypot(...q) - 1) < 1e-12);
  assert.ok(Math.abs(q[1] - Math.SQRT1_2) < 1e-12);
  assert.ok(Math.abs(q[3] - Math.SQRT1_2) < 1e-12);
  assert.throws(() => lookRotation([0, 0, 0], [0, 0, 0]), /Degenerate/);
});

test('NVML unavailable values stay null and literal zero stays measured zero', async () => {
  const {parseNvmlCsv}=await helpers;
  const row=parseNvmlCsv('2026/10/26 21:00:00.000, GPU-example, NVIDIA Example, 00000000:01:00.0, 610.88, 700, 8192, 0, 8001, 48, [N/A]');
  assert.equal(row.memoryUsedMiB,700);assert.equal(row.graphicsClockMHz,0);assert.equal(row.powerW,null);
  assert.throws(()=>parseNvmlCsv('unavailable'),/Unexpected/);
});
