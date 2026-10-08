"""Read-only production-script audit. Every input is synthetic and is NOT match evidence."""
import copy, hashlib, importlib.util, json, math, subprocess, sys
from pathlib import Path
from datetime import datetime, timezone
project=Path(__file__).resolve().parents[2];script=project/'game/v6/summarize_balance.py'
stamp=datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ');out=Path(__file__).resolve().parent/f'balance-summary-audit-{stamp}';out.mkdir()
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
source_hash=digest(script);spec=importlib.util.spec_from_file_location('production_balance_summary',script);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
branches=('speed','security','algorithm','science','lightweight');matrix=[]
# Independent enumeration follows the actual Rust runner's nested loops.
for blue in branches:
 for red in branches:
  for seed_offset in range(20):
   for swapped in (False,True):
    matrix.append({'syntheticFixture':True,'fixturePurpose':'Statistics unit test only, no simulation executed','index':len(matrix),'seed':1000+seed_offset,'originalPair':[blue,red],'swapped':swapped,'branches':[red,blue] if swapped else [blue,red],'strategies':['mixed-ai','mixed-ai'],'theme':('river','mining','highland')[seed_offset%3],'winner':1,'seconds':2100,'winReason':'SYNTHETIC','firstT5Seconds':[None,None],'secondT5Seconds':[None,None],'lostValue':[10,20],'simulationSourceSha256':'a'*64,'rulesVersion':'SYNTHETIC-V6','playabilityWarnings':{},'orderMetrics':{}})
checks=[];findings=[];cli=[]
def check(name,condition,details=None):
 checks.append({'name':name,'pass':bool(condition),'details':details})
 if not condition:findings.append({'severity':'review','name':name,'details':details})
def run_cli(name,rows,*extra):
 input_path=out/f'{name}.synthetic.jsonl';output=out/f'{name}.synthetic.report.json';input_path.write_text(''.join(json.dumps(v)+'\n' for v in rows),encoding='utf-8')
 command=[sys.executable,str(script),str(input_path),'--output',str(output),*extra];p=subprocess.run(command,capture_output=True,text=True);data=json.loads(output.read_text(encoding='utf-8')) if output.exists() else None
 record={'name':name,'exitCode':p.returncode,'outputExists':output.exists(),'integrityPassed':data.get('integrityPassed') if data else None,'input':input_path.name,'output':output.name,'stderr':p.stderr[-1500:]};cli.append(record);return p,data,command,output
p,full,command,full_path=run_cli('full-canonical-matrix',matrix,'--expected-games','1000','--require-branch-matrix')
check('independent-1000-index-enumeration',p.returncode==0 and full['integrityPassed'])
check('game-count-is-not-player-perspective-count',full['games']==1000 and full['durationSeconds']['samples']==1000 and all(v['playerObservations']==400 for v in full['branches'].values()) and all(v['playerObservations']==80 for v in full['branchMatchups'].values()),{'branchObservations':{k:v['playerObservations'] for k,v in full['branches'].items()},'matchupGroups':len(full['branchMatchups'])})
check('mirror-branch-and-spawn-side-accounting',all(v['wins']==200 and v['losses']==200 and v['winFractionCompleted']==.5 for v in full['branches'].values()) and full['spawnSides']['1']['wins']==1000 and full['spawnSides']['2']['wins']==0,{'scope':'Synthetic side1 wins every game; faction rates must stay.5 while side bias remains visible'})
check('theme-mapping-by-seed-not-case-parity',{k:v['playerObservations'] for k,v in full['themes'].items()}=={'river':700,'mining':700,'highland':600})
check('integrity-never-manufactures-final-balance-pass',full['finalBalanceAcceptance'] is False)
boundary_indices=[0,1,38,39,40,41,198,199,200,201,998,999]
check('enumeration-boundaries',all(matrix[i]['index']==i and matrix[i]['seed']==1000+(i//2)%20 and matrix[i]['originalPair']==[branches[i//200],branches[(i//40)%5]] for i in boundary_indices),[matrix[i] for i in boundary_indices])
for field,value in [('seed',1001),('swapped',False),('originalPair',['security','speed']),('branches',['speed','security']),('theme','highland'),('strategies',['mech','mixed-ai'])]:
 changed=copy.deepcopy(matrix);changed[41][field]=value;res=module.aggregate(changed,1000,True);check('reject-wrong-matrix-'+field,not res['integrityPassed'],res['integrityIssues'])
self_rows=copy.deepcopy(matrix[:2]);self_report=module.aggregate(self_rows)
check('self-match-each-player-once',self_report['branches']['speed']['playerObservations']==4 and self_report['branches']['speed']['wins']==2 and self_report['branches']['speed']['losses']==2 and self_report['branchMatchups']['speed/speed']['winFractionCompleted']==.5)
mirror=copy.deepcopy(matrix[40:42]);mirror[0]['winner']=1;mirror[1]['winner']=2;mirror[0]['firstT5Seconds']=[1200,None];mirror[1]['firstT5Seconds']=[None,1300];mr=module.aggregate(mirror)
check('mirrored-owner-arrays-follow-actual-branches',mr['branches']['speed']['wins']==2 and mr['branches']['security']['wins']==0 and mr['branches']['speed']['metrics']['firstT5Seconds']['mean']==1250)
null_stat=mr['branches']['security']['metrics']['firstT5Seconds']
check('null-t5-is-missing-not-zero',null_stat['samples']==0 and null_stat['missing']==2 and 'mean' not in null_stat and mr['branches']['security']['firstT5Reached']==0 and mr['branches']['security']['secondT5Before32Minutes']==0,null_stat)
for name,rows in [('duplicate-index',[copy.deepcopy(matrix[0]),copy.deepcopy(matrix[0])]),('missing-fingerprint',[copy.deepcopy(matrix[0])]),('mixed-fingerprints',copy.deepcopy(matrix[:2]))]:
 if name=='missing-fingerprint':rows[0].pop('simulationSourceSha256')
 if name=='mixed-fingerprints':rows[1]['simulationSourceSha256']='b'*64
 p,res,_,_=run_cli(name,rows);check('cli-rejects-'+name,p.returncode!=0 and res is not None and not res['integrityPassed'])
before=digest(full_path);p=subprocess.run(command,capture_output=True,text=True);check('existing-output-preserved',p.returncode!=0 and digest(full_path)==before)
# Negative type probes expose false-positive integrity outcomes, without editing production.
bad=copy.deepcopy(matrix[0]);bad['rulesVersion']=None;p,res,_,_=run_cli('explicit-null-rules-version',[bad]);check('reject-explicit-null-rules-version',p.returncode!=0 and (res is None or not res['integrityPassed']),{'observedIntegrityPassed':res['integrityPassed'] if res else None,'reportedRulesVersions':res['rulesVersions'] if res else None})
bad=copy.deepcopy(matrix);bad[0]['index']=False;bad[1]['index']=True;res=module.aggregate(bad,1000,True);check('reject-json-boolean-matrix-indices',not res['integrityPassed'],{'observedIntegrityPassed':res['integrityPassed']})
bad=copy.deepcopy(matrix[0]);bad['winner']=True;res=module.aggregate([bad]);check('reject-json-boolean-winner',not res['integrityPassed'],{'observedIntegrityPassed':res['integrityPassed'],'winsBySide':res['winsBySide'],'completed':res['completed']})
# Exercise the new trade implementation with asymmetric, zero and missing losses.
if 'destroyedAssetExchange' in full['branches']['speed']:
 own_self=module.aggregate([copy.deepcopy(matrix[0])])['branches']['speed']['destroyedAssetExchange']
 check('self-trade-aggregate-is-one-not-mean-of-reciprocals',own_self['aggregateEnemyOverOwnRatio']==1 and own_self['perPlayerRatioDistribution']['mean']==1.25,own_self)
 zero=copy.deepcopy(matrix[0]);zero['lostValue']=[0,0];zr=module.aggregate([zero])['branches']['speed']['destroyedAssetExchange']
 check('zero-zero-trade-remains-null',zr['aggregateEnemyOverOwnRatio'] is None and zr['bothZero']==2 and zr['ownZeroEnemyPositive']==0 and zr['perPlayerRatioDistribution']['samples']==0 and zr['perPlayerRatioDistribution']['missing']==2 and 'mean' not in zr['perPlayerRatioDistribution'],zr)
 zero=copy.deepcopy(matrix[40]);zero['lostValue']=[0,20];zr=module.aggregate([zero])['branches']['speed']['destroyedAssetExchange']
 check('zero-own-positive-enemy-counted-separately',zr['aggregateEnemyOverOwnRatio'] is None and zr['bothZero']==0 and zr['ownZeroEnemyPositive']==1 and zr['perPlayerRatioDistribution']['samples']==0,zr)
 paired=copy.deepcopy([matrix[40],matrix[42]]);paired[1]['lostValue']=[None,1000];pr=module.aggregate(paired)['branches']['speed']['destroyedAssetExchange']
 check('trade-numerator-and-denominator-use-same-valid-pairs',pr['recordedPairs']==1 and pr['missingPairs']==1 and pr['ownLossTotal']==10 and pr['enemyLossTotal']==20 and pr['aggregateEnemyOverOwnRatio']==2,pr)
 bad=copy.deepcopy(matrix[40]);bad['lostValue']=[-1,20];pr=module.aggregate([bad])['branches']['speed']['destroyedAssetExchange']
 check('negative-asset-loss-pair-is-not-a-valid-trade',pr['recordedPairs']==0 and pr['missingPairs']==1 and pr['aggregateEnemyOverOwnRatio'] is None,pr)
 bad=copy.deepcopy(matrix[40]);bad['lostValue']=[False,20];pr=module.aggregate([bad])['branches']['speed']['destroyedAssetExchange']
 check('boolean-loss-is-not-zero-value',pr['recordedPairs']==0 and pr['missingPairs']==1,pr)
# This demonstrates the proposed exchange-ratio pitfall, not a game outcome.
ratios=[20/10,10/20];trade={'syntheticLosses':[10,20],'twoPlayerRatios':ratios,'meanOfPlayerRatios':sum(ratios)/2,'ratioOfSummedEnemyToOwnLoss':(20+10)/(10+20),'zeroOwnLossPolicy':'null, with bothZero and ownZeroEnemyPositive counted separately','scope':'Per-observation distributions are fine, but their mean is not the neutral aggregate trade ratio for mirrored self-play.'}
check('production-source-unmodified',digest(script)==source_hash)
result={'scope':'Synthetic statistical unit fixtures only. No native match, GPU, economy or balance simulation was executed. Production summarizer was not edited.','sourceFile':str(script),'sourceSha256':source_hash,'at':datetime.now(timezone.utc).isoformat(),'checks':checks,'cliRuns':cli,'findings':findings,'tradeMetricReview':trade,'pass':not findings}
(out/'audit-result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8');(out/'README.txt').write_text('Every JSONL and report in this directory is SYNTHETIC UNIT-TEST DATA. Never merge these files into actual balance results.\n',encoding='utf-8');print(json.dumps({'output':str(out/'audit-result.json'),'checks':len(checks),'passed':sum(c['pass'] for c in checks),'findings':findings,'trade':trade},ensure_ascii=False))
