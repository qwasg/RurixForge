/** Actual private native catalogue observation; no renderer/performance claim. */
import assert from 'node:assert/strict';import fs from 'node:fs';import path from 'node:path';import {createHash} from 'node:crypto';
import {launchNative} from './native-rpc.mjs';
const [projectArg,executableArg,outArg]=process.argv.slice(2);assert.ok(projectArg&&executableArg&&outArg);
const root=path.resolve(projectArg),executable=path.resolve(executableArg),out=path.resolve(outArg);
assert.ok(!fs.existsSync(out),'Preserve previous host identity observations');fs.mkdirSync(out,{recursive:true});
const native=await launchNative({root,executable,logs:path.join(out,'process')});let report;
try{const catalog=await native.rpc('game.session.catalog');assert.equal(catalog.version,6);assert.match(catalog.rulesFingerprint,/^[a-f0-9]{64}$/);assert.ok(catalog.rulesVersion);
 const file=path.join(out,'catalog.json');fs.writeFileSync(file,JSON.stringify(catalog,null,2));
 report={kind:'native-host-identity',observedAt:new Date().toISOString(),actualNativeExecution:true,nativePid:native.pid,executable,engineSha256:native.rulesHash,rulesVersion:catalog.rulesVersion,rulesFingerprint:catalog.rulesFingerprint,observedMethods:['game.session.catalog'],catalog:{path:path.relative(root,file).replaceAll('\\','/'),sha256:createHash('sha256').update(fs.readFileSync(file)).digest('hex')},scope:'Normally started exact local executable, observed native catalogue. No game state, graphics or performance acceptance is inferred.'};
}finally{await native.close();}
fs.writeFileSync(path.join(out,'host-identity.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
