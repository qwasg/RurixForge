/** Checks the prepared soak assertions against snapshots of a real earned LAN save. */
import fs from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';
import {launchNative} from './native-rpc.mjs';import {verifyOwnerView} from './view-contract.mjs';
const dir=path.dirname(fileURLToPath(import.meta.url)),root=path.resolve(dir,'../..');
const saveFile=process.argv[2];if(!saveFile)throw new Error('Pass the actual earned save file');
const stored=JSON.parse(fs.readFileSync(saveFile,'utf8'));const native=await launchNative({root,executable:path.join(dir,'runtime-bin/engine-host.exe'),logs:path.join(root,'Logs/v6/owner-view-probe')});
try{
 await native.rpc('game.session.open',{mode:'authority',seed:stored.save.snapshot.seed,theme:stored.save.snapshot.theme,opponent:'human'});
 await native.rpc('game.session.load',{save:stored.save});await native.rpc('play.pause');
 const views=[];for(const owner of[1,2]){const snapshot=await native.rpc('game.session.snapshot',{owner});verifyOwnerView(snapshot,owner);views.push({owner,tick:snapshot.tick,buildings:snapshot.buildings.length,rooms:snapshot.rooms.length,units:snapshot.units.length,visible:snapshot.visible[owner-1].length});}
 const report={at:new Date().toISOString(),passed:true,scope:'Prepared observer assertions against actual native owner snapshots from a real paid LAN save; no fabricated resources, not a new LAN-duration or performance claim',engineSha256:native.rulesHash,saveFile,views};fs.writeFileSync(path.join(dir,'owner-view-native-20260911.json'),JSON.stringify(report,null,2));console.log(JSON.stringify(report));
}finally{await native.close();}
