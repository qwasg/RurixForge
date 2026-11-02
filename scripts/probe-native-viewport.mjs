/** Real native output probe. Attaches to an already running, explicitly selected host.
 * node scripts/probe-native-viewport.mjs --port 12345 --presenter C:/.../viewport-presenter.exe --out E:/.../new-run --play
 * Does not launch/build an engine, alter source assets, or claim GPU timestamps are display times.
 */
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { trajectoryPose, parseNvmlCsv, nvmlQueryFields } from './native-probe-contract.mjs';

const args = process.argv.slice(2);
const arg = (name, fallback) => { const i = args.indexOf(`--${name}`); return i < 0 ? fallback : args[i + 1]; };
const port = Number(arg('port', 0));
const presenterPath = path.resolve(arg('presenter', ''));
const output = path.resolve(arg('out', ''));
const seconds = Number(arg('seconds', 20));
const runs = Number(arg('runs', 1));
const soak = Number(arg('soak-seconds', 0));
const width = Number(arg('width', 2560));
const height = Number(arg('height', 1440));
const maxFps=Number(arg('max-fps',240));
const vramBudgetMiB=Number(arg('vram-budget-mib',7.2*1024));
const expectedValidation=arg('validation','any');
const soakPort=Number(arg('soak-port',0));
if(!['any','enabled','disabled'].includes(expectedValidation))throw new Error('--validation must be enabled, disabled, or any');
if(args.includes('--soak')&&expectedValidation!=='enabled')throw new Error('--soak requires --validation enabled on a separate correctness host');
if(soak>0&&!soakPort)throw new Error('A mixed performance/validation campaign requires --soak-port for a separate validation-enabled host; or run --single-run --soak --seconds 1200 separately');
if (!port || !args.includes('--out') || !args.includes('--presenter') || !fs.existsSync(presenterPath)
  || maxFps < 1 || maxFps > 240 || seconds < 2 || seconds > (args.includes('--soak') ? 1200 : 120) || !Number.isInteger(runs) || runs < 1 || runs > 3 || soak < 0 || soak > 1200 || width < 16 || width > 2560 || height < 16 || height > 1440) {
  throw new Error('Require --port, existing --presenter, new --out; --seconds 2..120, output up to 2560x1440');
}
if (fs.existsSync(output)) throw new Error('Output must not exist; evidence is append only');
fs.mkdirSync(output, { recursive: true });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const sha = (file) => createHash('sha256').update(fs.readFileSync(file)).digest('hex');
// Separate processes and evidence folders for the three runs and the optional soak.
if (!args.includes('--single-run') && (runs > 1 || soak > 0)) {
  const manifest = { schema:'forge.native.viewport.campaign.v1',startedUtc:new Date().toISOString(),runs:[],passed:true };
  const jobs = [...Array.from({length:runs},(_,i)=>({name:`run-${i+1}`,seconds})), ...(soak ? [{name:'soak',seconds:soak,soak:true}] : [])];
  for (const job of jobs) {
    const childArgs=[];
    for(let i=0;i<args.length;i++){if(['--out','--seconds','--runs','--soak-seconds','--soak-port','--port','--validation'].includes(args[i])){i++;continue;}childArgs.push(args[i]);}
    childArgs.push('--single-run','--out',path.join(output,job.name),'--seconds',String(job.seconds),'--port',String(job.soak?soakPort:port),'--validation',job.soak?'enabled':'disabled');
    if(job.soak)childArgs.push('--soak');
    if(!childArgs.includes('--contract'))childArgs.push('--contract',path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../projects/aaa-scene-lab/SceneContract.json'));
    const code=await new Promise((resolve)=>{const child=spawn(process.execPath,[fileURLToPath(import.meta.url),...childArgs],{windowsHide:true,stdio:'inherit'});child.on('error',()=>resolve(-1));child.on('exit',resolve);});
    const resultPath=path.join(output,job.name,'native-viewport.json');
    const result=fs.existsSync(resultPath)?JSON.parse(fs.readFileSync(resultPath,'utf8')):null;
    manifest.runs.push({...job,exitCode:code,report:resultPath,sha256:fs.existsSync(resultPath)?sha(resultPath):null,nativeContractPassed:result?.nativeContractPassed??false,target60GateMet:result?.target60GateMet??false,soakGateMet:result?.soakGateMet??false});
    if(code!==0||(job.soak ? result?.soakGateMet!==true : result?.target60GateMet!==true)){manifest.passed=false;break;}
  }
  manifest.finishedUtc=new Date().toISOString();fs.writeFileSync(path.join(output,'campaign.json'),JSON.stringify(manifest,null,2),{flag:'wx'});
  process.exit(manifest.passed?0:1);
}
let requestId = 0;
let controlSocket;
let controlReady;
let controlBytes=Buffer.alloc(0);
const pending=new Map();
function connectControl(){
  if(controlSocket&&!controlSocket.destroyed)return controlReady;
  controlBytes=Buffer.alloc(0);
  controlSocket=net.connect({host:'127.0.0.1',port});controlSocket.setNoDelay(true);
  controlReady=new Promise((resolve,reject)=>{controlSocket.once('connect',resolve);controlSocket.once('error',reject);});
  controlSocket.on('error',(error)=>{for(const item of pending.values()){clearTimeout(item.timer);item.reject(error);}pending.clear();});
  controlSocket.on('data',(bytes)=>{
    controlBytes=Buffer.concat([controlBytes,bytes]);
    while(controlBytes.length>=4){
      const size=controlBytes.readUInt32LE(0);if(size>8*1024*1024){controlSocket.destroy(new Error('Control reply exceeded 8 MiB'));return;}
      if(controlBytes.length<size+4)return;
      const value=JSON.parse(controlBytes.subarray(4,size+4));controlBytes=controlBytes.subarray(size+4);
      const item=pending.get(value.id);if(!item)continue;pending.delete(value.id);clearTimeout(item.timer);
      if(value.error)item.reject(new Error(`${item.method}: ${JSON.stringify(value.error)}`));else item.resolve(value.result);
    }
  });
  return controlReady;
}
async function rpc(method,params={}){
  await connectControl();
  return new Promise((resolve,reject)=>{
    const id=++requestId;const body=Buffer.from(JSON.stringify({jsonrpc:'2.0',id,method,params}));const prefix=Buffer.alloc(4);prefix.writeUInt32LE(body.length);
    const timer=setTimeout(()=>{pending.delete(id);controlSocket.destroy();reject(new Error(`${method}: timeout`));},30000);pending.set(id,{resolve,reject,timer,method});
    controlSocket.write(Buffer.concat([prefix,body]));
  });
}
const contractPath = arg('contract',null);
const contract=contractPath?JSON.parse(fs.readFileSync(contractPath,'utf8')):null;

const report = { schema: 'forge.native.viewport.probe.v1', startedUtc: new Date().toISOString(),
  classification: 'unlocked', requestedOutput: [width, height], frameGeneration: false, requestedMaxFps:maxFps,validationExpected:expectedValidation,
  presenter: { path: presenterPath, sha256: sha(presenterPath) }, statuses: [], errors: [], nativeSamples: [], gpuSamples: [], trajectory: {updates:0,skippedTicks:0,cuts:[]}, binaryFrames: 0 };
let presenter;
let socket;
let shared = false;
let enteredPlay = false;
let ready = false;
let measurementDone=false;
let trajectoryTask;
let failure;
const samples = new Map();
const hostTimingFields=['streamFrameId','generation','slot','producerFrame','signalSubmittedUtcMicros','snapshotLockWaitMs','snapshotBuildMs','outputLeaseWaitMs','consumerWaitMs','rendererCallMs','signalSubmitMs','metricsCloneMs','previousLimiterSleepMs','bookkeepingLockWaitMs','bookkeepingTotalMs','loopBeforePublishMs'];
const hostTimingSamples=[];
const gpuFrameMs=[];
const gpuEvents={frames:0,gaps:0,lastFrameId:null,historyResets:0,validationPositiveFrames:0,maxValidationErrors:0,maxAllocatedBytes:0,contractViolations:0,timingUnavailable:0,validationEnabledFrames:0,validationMissingFrames:0,validationModeMismatchFrames:0};
let lastGpuSnapshot=0;
const maxSamples=Math.ceil(seconds*maxFps)+1024;
const logs = [];
let nvmlProcess, nvmlLog, nvmlClosed, nvmlBuffer='';
const nvmlRows=[];const nvmlState={queryFields:nvmlQueryFields,intervalSeconds:1,rawCsv:path.join(output,'nvml-raw.csv'),stderr:'',parseErrors:0,stoppedByProbe:false};
function startNvml(){
  const args=[`--query-gpu=${nvmlQueryFields.join(',')}`,'--format=csv,noheader,nounits','-l','1'];
  const requestedUuid=arg('gpu-uuid',null);if(requestedUuid)args.push('-i',requestedUuid);
  nvmlState.command=['nvidia-smi',...args];
  nvmlLog=fs.createWriteStream(nvmlState.rawCsv,{flags:'wx'});
  nvmlLog.on('error',(error)=>{nvmlState.logError=String(error);});
  nvmlProcess=spawn('nvidia-smi',args,{windowsHide:true,stdio:['ignore','pipe','pipe']});
  nvmlClosed=new Promise((resolve)=>{nvmlProcess.once('close',(code,signal)=>{nvmlState.exitCode=code;nvmlState.signal=signal;resolve();});nvmlProcess.once('error',(error)=>{nvmlState.error=String(error);resolve();});});
  nvmlProcess.stdout.on('data',(bytes)=>{
    nvmlLog.write(bytes);nvmlBuffer+=bytes.toString();
    while(nvmlBuffer.includes('\n')){const end=nvmlBuffer.indexOf('\n');const line=nvmlBuffer.slice(0,end).trim();nvmlBuffer=nvmlBuffer.slice(end+1);if(!line)continue;
      try{const row=parseNvmlCsv(line);if(nvmlRows.length<100000)nvmlRows.push({receivedUtc:new Date().toISOString(),...row});else nvmlState.error='NVML row budget exceeded';}
      catch{nvmlState.parseErrors++;}
    }
  });
  nvmlProcess.stderr.on('data',(bytes)=>{nvmlState.stderr=(nvmlState.stderr+bytes.toString()).slice(0,8192);});
}
async function stopNvml(){
  if(!nvmlProcess)return;
  nvmlState.stoppedByProbe=true;
  if(nvmlProcess.exitCode===null&&nvmlProcess.signalCode===null)nvmlProcess.kill();
  await Promise.race([nvmlClosed,sleep(2000)]);
  await new Promise((resolve)=>nvmlLog.end(resolve));
  nvmlProcess=null;
}
function nvmlSummary(){
  const requestedUuid=arg('gpu-uuid',null);const deviceName=report.realtimeMetrics?.device;
  const matches=nvmlRows.filter((row)=>requestedUuid?row.uuid===requestedUuid:row.name===deviceName);
  const uuids=[...new Set(matches.map((row)=>row.uuid))];
  const bound=uuids.length===1&&matches.every((row)=>row.name===deviceName);
  const used=bound?matches.map((row)=>row.memoryUsedMiB).filter(Number.isFinite):[];
  const peak=used.length?Math.max(...used):null;
  return {...nvmlState,scope:'Full-device NVML usage sampled once per second; includes other processes and can miss shorter transient peaks',rendererDeviceName:deviceName??null,
    deviceBinding:bound?{uuid:uuids[0],method:requestedUuid?'explicit UUID plus renderer name equality':'unique NVML renderer-name match'}:null,
    sampleCount:matches.length,sampledPeakMemoryMiB:peak,vramBudgetMiB,memoryBudgetMet:peak===null?null:peak<=vramBudgetMiB,
    available:bound&&used.length>0&&!nvmlState.error&&!nvmlState.logError&&nvmlState.parseErrors===0,samples:matches};
}

const bind = (share) => {
  if (share.protocol !== 2 || share.slots?.length !== 2 || share.width !== width || share.height !== height) {
    throw new Error('Native output negotiation did not preserve requested extent and two slots');
  }
  const handles = share.slots.flatMap((s) => [s.texHandle, s.fenceHandle, s.consumerFenceHandle]);
  presenter.stdin.write(`bind buf2 ${share.generation} ${handles.join(' ')} ${share.width} ${share.height} ${share.rowPitch}\n`);
};
try {
  report.capabilities = await rpc('render.capabilities');
  if (!report.capabilities.frameExits.nativeOnly) throw new Error('Host did not enable native-only Rurix output');
  if(contract&&!report.capabilities.runtimeTransforms)throw new Error('Host does not provide the transient trajectory input API');
  report.initialRealtimeStats=await rpc('viewport.renderStats');
  if(expectedValidation!=='any'&&report.initialRealtimeStats.validationRequested!==(expectedValidation==='enabled'))throw new Error('Host validation configuration does not match the requested measurement profile');
  if(contract){const expectedScene=path.resolve(path.dirname(contractPath),'Content/Scenes/Main.rxscene');if(path.resolve(report.capabilities.scenePath??'').toLowerCase()!==expectedScene.toLowerCase())throw new Error('Host scene does not match the supplied SceneContract project');}
  presenter = spawn(presenterPath, ['--standalone', '--high-performance', '--record-metrics'], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] });
  let text = '';
  presenter.stdout.on('data', (bytes) => {
    text += bytes.toString();
    while (text.includes('\n')) {
      const end = text.indexOf('\n'); const line = text.slice(0, end).trim(); text = text.slice(end + 1);
      if (line === 'PRESENTER_READY') ready = true;
      else if (line.startsWith('PRESENT_STATS ')) {
        const stats = JSON.parse(line.slice('PRESENT_STATS '.length));
        report.presentCalls = stats.presentCalls; report.visiblePresents=stats.visiblePresents;
        for (const sample of stats.samples) samples.set(`${sample.generation}:${sample.sample}`, sample);
        if(samples.size>maxSamples){failure=new Error('Native samples exceeded bounded run budget');measurementDone=true;}
      } else if (logs.length < 200) logs.push(line);
    }
  });
  presenter.stderr.on('data', (bytes) => { if (logs.length < 200) logs.push(bytes.toString().slice(0, 4096)); });
  presenter.on('error', (error) => { failure = error; });
  const deadline = Date.now() + 10000;
  while (!ready && !failure && Date.now() < deadline && presenter.exitCode === null) await sleep(25);
  if (!ready) throw failure ?? new Error('Presenter did not become ready');
  const share = await rpc('viewport.shareOpen', { pid: presenter.pid, protocol: 2, width, height });
  shared = true; report.outputGeneration = share.generation; bind(share);
  if (contract) { report.sceneContract={path:path.resolve(contractPath),sha256:sha(contractPath),schema:contract.schema,tickRate:contract.tickRate,durationSeconds:contract.durationSeconds}; }
  if (args.includes('--play') || contract) {
    const state = await rpc('play.state');
    if (state.state === 'edit') { await rpc('play.enter'); enteredPlay = true; }
  }
  const info = await rpc('viewport.streamInfo');
  socket = new WebSocket(info.wsUrl); socket.binaryType = 'arraybuffer';
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Native WebSocket open timed out')), 5000);
    socket.addEventListener('open', () => { clearTimeout(timer); resolve(); }, { once: true });
    socket.addEventListener('error', () => { clearTimeout(timer); reject(new Error('Native WebSocket failed')); }, { once: true });
  });
  socket.addEventListener('message', (event) => {
    if (typeof event.data !== 'string') { report.binaryFrames++; return; }
    const value = JSON.parse(event.data);
    if (value.type === 'frameMetrics' && !measurementDone) {
      if(gpuEvents.frames>=maxSamples){failure=new Error('GPU samples exceeded bounded run budget');measurementDone=true;return;}
      const m=value.metrics;
      if(args.includes('--diagnostics')){
        const timing={streamFrameId:value.frameId,...m.nativeFrame,...m.hostTimings};hostTimingSamples.push(hostTimingFields.map((field)=>timing[field]??null));
      }
      if(gpuEvents.lastFrameId!==null && value.frameId!==gpuEvents.lastFrameId+1)gpuEvents.gaps++;
      gpuEvents.lastFrameId=value.frameId;gpuEvents.frames++;
      gpuEvents.historyResets+=Number(!!m.historyReset);
      gpuEvents.validationEnabledFrames+=Number(m.validationMessengerInstalled===true);
      gpuEvents.validationMissingFrames+=Number(m.validationRequested===true&&m.validationMessengerInstalled!==true);
      gpuEvents.validationModeMismatchFrames+=Number(expectedValidation==='enabled'?(m.validationRequested!==true||m.validationMessengerInstalled!==true):expectedValidation==='disabled'?(m.validationRequested!==false||m.validationMessengerInstalled!==false):false);
      gpuEvents.validationPositiveFrames+=Number((m.validationErrors??0)>0);
      gpuEvents.maxValidationErrors=Math.max(gpuEvents.maxValidationErrors,m.validationErrors??0);
      gpuEvents.maxAllocatedBytes=Math.max(gpuEvents.maxAllocatedBytes,m.allocatedBytes??0);
      gpuEvents.contractViolations+=Number(m.nativeOutput!==true||m.pixelReadback!==false||m.outputExtent?.[0]!==width||m.outputExtent?.[1]!==height||!Array.isArray(m.internalExtent));
      if(!Number.isFinite(m.gpuMs))gpuEvents.timingUnavailable++;
      gpuFrameMs.push(Number.isFinite(m.gpuMs)?m.gpuMs:null);
      if(performance.now()-lastGpuSnapshot>=1000){
        lastGpuSnapshot=performance.now();
        report.gpuSamples.push({streamFrameId:value.frameId,frame:m.frame,outputExtent:m.outputExtent,internalExtent:m.internalExtent,renderScale:m.renderScale,gpuMs:m.gpuMs,cpuRecordMs:m.cpuRecordMs,cpuSubmitMs:m.cpuSubmitMs,cpuWaitMs:m.cpuWaitMs,adapterWallMs:m.adapterWallMs,sceneUpdateMs:m.sceneUpdateMs,allocatedBytes:m.allocatedBytes,historyReset:m.historyReset,validationErrors:m.validationErrors,pixelReadback:m.pixelReadback,nativeOutput:m.nativeOutput,validationRequested:m.validationRequested,validationMessengerInstalled:m.validationMessengerInstalled,validationErrorTotal:m.validationErrorTotal,hostTimings:m.hostTimings,nativeFrame:m.nativeFrame,memoryHeaps:m.memoryHeaps,gpuPassSumMs:m.gpuPassSumMs,gpuTimingScope:m.gpuTimingScope});
      }
    }
    if (value.type === 'status' && report.statuses.length < 1300) report.statuses.push(value);
    if (value.type === 'error' && report.errors.length < 150) report.errors.push(value);
  });
  startNvml();
  socket.send(JSON.stringify({ type: 'subscribe', transport: 'native', width, height, maxFps, metrics:true }));
  const measurementStart=performance.now();
  report.measurementStartUtcMicros=Date.now()*1000;
  if(contract){trajectoryTask=(async()=>{
    let previousTick=-1,previousCycle=0;
    while(!measurementDone){
      const elapsed=(performance.now()-measurementStart)/1000;const tick=Math.floor(elapsed*contract.tickRate);
      if(tick>previousTick){
        if(previousTick>=0)report.trajectory.skippedTicks+=Math.max(0,tick-previousTick-1);
        const pose=trajectoryPose(contract,tick/contract.tickRate);
        await rpc('runtime.transforms',{updates:pose.updates});report.trajectory.updates++;
        if(pose.cycle!==previousCycle){report.trajectory.cuts.push({tick,cycle:pose.cycle});previousCycle=pose.cycle;}
        previousTick=tick;
      }
      await sleep(1);
    }
  })().catch((error)=>{failure=error;measurementDone=true;});}
  const until = measurementStart + seconds * 1000;
  while (performance.now() < until && presenter.exitCode === null && !measurementDone) { presenter.stdin.write('stat_json\n'); await sleep(250); }
  if(presenter.exitCode!==null&&performance.now()<until)failure=new Error(`Presenter exited before the requested measurement interval (code=${presenter.exitCode})`);
  measurementDone=true;report.measurementEndUtcMicros=Date.now()*1000;
  report.measurementWallSeconds=(performance.now()-measurementStart)/1000;
  if(trajectoryTask)await trajectoryTask;
  socket.close();
  await stopNvml();
  presenter.stdin.write('stat_json\n'); await sleep(100);
  report.realtimeMetrics = await rpc('viewport.renderStats');
  if(failure)throw failure;
  // Stop the timed native subscription before any pixel capture or readback.
  socket.close();await sleep(100);
  if (args.includes('--capture')) {
    const captureWarmup=Number(arg('capture-warmup',32));
    if(!Number.isInteger(captureWarmup)||captureWarmup<0||captureWarmup>120)throw new Error('--capture-warmup must be 0..120');
    const start=performance.now();
    for(let i=0;i<captureWarmup;i++)await rpc('viewport.frame',{width,height,format:'none',exact:true,camera:'scene'});
    report.capture = await rpc('viewport.frame', { width, height, format: 'file', exact: true, camera: 'scene' });
    report.capturePhase={excludedFromTiming:true,warmupFrames:captureWarmup,elapsedSeconds:(performance.now()-start)/1000};
  }
  report.nativeSamples = [...samples.values()].sort((a, b) => a.sample - b.sample);
  const warmup = Number(arg('warmup-frames', 60));
  const timedSamples=report.nativeSamples.filter((s)=>Number.isFinite(s.presentedUtcMicros)&&s.presentedUtcMicros>=report.measurementStartUtcMicros&&s.presentedUtcMicros<=report.measurementEndUtcMicros);
  report.presentationWindowTimestampAvailable=report.nativeSamples.every((s)=>Number.isFinite(s.presentedUtcMicros));
  const postWarmup=timedSamples.slice(warmup);
  const steady = postWarmup.filter((s) => s.intervalMs > 0 && s.windowVisible === true && s.occluded === false);
  const intervals = steady.map((s) => s.intervalMs).sort((a, b) => a - b);
  const percentile = (p) => intervals.length ? intervals[Math.min(intervals.length - 1, Math.floor((intervals.length - 1) * p))] : null;
  report.presentation = { sampleCount: steady.length, capturedSampleCount:report.nativeSamples.length,timedSampleCount:timedSamples.length,warmupFrames: warmup, intervalMs: { p50: percentile(.5), p95: percentile(.95), p99: percentile(.99) },
    fpsFromPresentIntervals: intervals.length ? 1000 * intervals.length / intervals.reduce((a, b) => a + b, 0) : null,
    dxgiStatisticsSamples: steady.filter((s) => s.dxgiPresentCount != null).length,
    invisibleOrOccludedSamples:postWarmup.filter((s)=>s.windowVisible!==true||s.occluded!==false).length,
    scope: 'Successful visible Present calls timed on presenter thread; DXGI display statistics included when available. Not GPU timestamps.' };
  const metricGaps=gpuEvents.gaps;
  const presentGaps=report.nativeSamples.slice(1).filter((m,i)=>m.sample!==report.nativeSamples[i].sample+1).length;
  const gpuTimes=gpuFrameMs.slice(warmup).filter(Number.isFinite).sort((a,b)=>a-b);
  const gpuP=(p)=>gpuTimes.length?gpuTimes[Math.min(gpuTimes.length-1,Math.floor((gpuTimes.length-1)*p))]:null;
  report.gpuSummary={scope:"Sum of per-pass GPU timestamps; work outside the timed passes is not included",...gpuEvents,sampleCount:gpuTimes.length,frameMetricGaps:metricGaps,presentationSampleGaps:presentGaps,p50Ms:gpuP(.5),p95Ms:gpuP(.95),p99Ms:gpuP(.99)};
  report.passed = report.measurementWallSeconds>=seconds && timedSamples.length > warmup && report.presentationWindowTimestampAvailable && gpuTimes.length>0 && gpuEvents.contractViolations===0 && gpuEvents.timingUnavailable===0 && metricGaps===0 && presentGaps===0 && report.presentation.invisibleOrOccludedSamples===0 && gpuEvents.maxValidationErrors===0 && gpuEvents.validationMissingFrames===0 && gpuEvents.validationModeMismatchFrames===0 && report.binaryFrames === 0 && report.errors.length === 0
    && new Set(report.nativeSamples.map((s) => s.slot)).size === 2
    && report.nativeSamples.every((s) => s.width === width && s.height === height)
    && report.realtimeMetrics.nativeOutput === true && report.realtimeMetrics.pixelReadback === false;
  report.nativeContractPassed=report.passed;
  report.target60P95Met = report.passed && report.presentation.intervalMs.p95 <= 1000 / 60 && report.presentation.fpsFromPresentIntervals >= 60;
  if(args.includes('--require-60'))report.passed=report.target60P95Met;
  report.gpuPassSum60P95BudgetMet=report.gpuSummary.p95Ms!==null&&report.gpuSummary.p95Ms<=1000/60;
  report.capacityInterpretation=report.gpuPassSum60P95BudgetMet&&!report.target60P95Met?'Timed passes total fits 16.67ms but visible intervals do not; GPU work outside passes, CPU work, display refresh, and scheduling still contribute. Pass sum alone does not establish frame capacity.':'See independent GPU and visible presentation distributions';
} catch (error) { report.passed = false; report.failure = String(error?.stack ?? error); }
finally {
  measurementDone=true;if(trajectoryTask)await trajectoryTask;
  await stopNvml();
  socket?.close(); await sleep(100);
  if (shared) { try { report.close = await rpc('viewport.shareClose'); } catch (error) { report.closeError = String(error); report.passed = false; } }
  if (enteredPlay) { try { await rpc('play.exit'); } catch (error) { report.playExitError = String(error); } }
  if (presenter && presenter.exitCode === null) {
    presenter.stdin.write('close\n'); await sleep(500);
    if (presenter.exitCode === null) presenter.kill();
  }
  controlSocket?.destroy();
  report.nativeContractPassed=!!report.nativeContractPassed && !report.closeError;
  report.nvml=nvmlSummary();
  report.presentationBudgets={minimumMeanFps:60,maximumP95Ms:1000/60,maximumP99Ms:25};
  report.target60GateMet=report.nativeContractPassed&&report.target60P95Met===true&&report.presentation?.intervalMs?.p99<=25&&report.nvml.available&&report.nvml.memoryBudgetMet===true&&expectedValidation==='disabled'&&gpuEvents.validationEnabledFrames===0;
  report.soakGateMet=args.includes('--soak')&&seconds>=1200&&report.nativeContractPassed&&report.nvml.available&&report.nvml.memoryBudgetMet===true&&expectedValidation==='enabled'&&gpuEvents.validationMissingFrames===0&&report.measurementWallSeconds>=seconds;
  if(args.includes('--require-60'))report.passed=report.target60GateMet;
  if(args.includes('--soak'))report.passed=report.soakGateMet;
  report.logs = logs; report.finishedUtc = new Date().toISOString();
  report.gpuEvents={...gpuEvents};report.gpuFrameMs=gpuFrameMs;
  if(args.includes('--diagnostics')){report.hostTimingFields=hostTimingFields;report.hostTimingSamples=hostTimingSamples;}
  report.gpuSnapshotCadence='at most one full summary per second; scalar GPU time and event counts retained for every completed frame';
  report.nativeSampleFields=['sample','generation','slot','producerFrame','width','height','copyCompleteMs','presentCallMs','intervalMs','dxgiPresentCount','dxgiSyncQpcTime','windowVisible','occluded','observedUtcMicros','pollingSleepMs','pollingIterations','presentedUtcMicros'];
  report.nativeSamples=[...samples.values()].sort((a,b)=>a.sample-b.sample).map((sample)=>report.nativeSampleFields.map((field)=>sample[field]??null));
  // Compact rows keep the full two-minute/soak corpus below the bounded evidence budget.
  fs.writeFileSync(path.join(output, 'native-viewport.json'), JSON.stringify(report), { flag: 'wx' });
}
console.log(JSON.stringify({ output, passed: report.passed, target60P95Met: report.target60P95Met, target60GateMet:report.target60GateMet,soakGateMet:report.soakGateMet, presentation: report.presentation, failure: report.failure }));
process.exitCode = report.passed ? 0 : 1;
