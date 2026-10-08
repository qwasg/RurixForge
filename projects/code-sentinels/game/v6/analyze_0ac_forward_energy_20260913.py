"""Read final energy-defense/workshop placement and accepted build timestamps."""
from pathlib import Path
import collections
import datetime
import hashlib
import json
import math
import summarize_balance

ROOT=Path(__file__).resolve().parents[2]
BASE=ROOT/'game/v6/final-balance-0acffa83-20260913/branch'
OUT=ROOT/'game/v6/causal-review-0acffa83-20260913'
read=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
ref=lambda p:dict(path=p.relative_to(ROOT).as_posix(),sha256=sha(p))


def main():
    observations=[]
    for i in range(1000):
        row=read(BASE/f'results-{i:03}.jsonl');s=read(BASE/f'diagnostics/match-{i}.save.json');state=s['snapshot']
        assert s['rulesFingerprint']==row['simulationSourceSha256'] and state['tick']/60==row['seconds']
        for owner in (1,2):
            rooms=[r for r in state['rooms'] if r['owner']==owner and r['kind']=='energy-defense' and r['hp']>0]
            build_orders=[o for o in s['orders'] if o['receipt']['accepted'] and o['order']['owner']==owner and o['order']['command']['op']=='room' and o['order']['command'].get('kind')=='energy-defense']
            laboratories=[o for o in s['orders'] if o['receipt']['accepted'] and o['order']['owner']==owner and o['order']['command']['op']=='room' and o['order']['command'].get('kind')=='research-lab']
            initial_lab=laboratories[0]['order']['command']['rect'] if laboratories else None
            for room in rooms:
                if initial_lab:
                    a=room['rect'];b=initial_lab
                    room['distanceFromFirstLabCenter']=math.hypot(a['x']+a['w']/2-b['x']-b['w']/2,a['y']+a['h']/2-b['y']-b['h']/2)
            observations.append(dict(index=i,owner=owner,branch=row['branches'][owner-1],opponent=row['branches'][2-owner],seed=row['seed'],winner=row['winner'],seconds=row['seconds'],nodeControlSeconds=row['nodeControlSeconds'][owner-1],nativeLostValue=row['lostValue'][owner-1],acceptedEnergyRoomOrders=[dict(tick=o['tick'],seconds=o['tick']/60,command=o['order']['command'])for o in build_orders],finalLivingEnergyRooms=rooms,finalPoweredRooms=sum(r['powered'] for r in rooms),finalOnlineRooms=sum(r['online'] for r in rooms),initialLabOrderRect=initial_lab))
        if (i+1)%250==0:print(json.dumps(dict(scanned=i+1)),flush=True)
    groups={}
    for branch in summarize_balance.BRANCHES:
        values=[o for o in observations if o['branch']==branch and o['opponent']!=branch]
        groups[branch]=dict(observations=len(values),everOrderedEnergyRoom=sum(bool(o['acceptedEnergyRoomOrders'])for o in values),acceptedEnergyRoomOrderCount=summarize_balance.stats(len(o['acceptedEnergyRoomOrders'])for o in values),firstAcceptedEnergyRoomOrderSeconds=summarize_balance.stats(o['acceptedEnergyRoomOrders'][0]['seconds']if o['acceptedEnergyRoomOrders']else None for o in values),finalLivingRoomCounts=dict(collections.Counter(len(o['finalLivingEnergyRooms'])for o in values)),finalPoweredRoomCount=summarize_balance.stats(o['finalPoweredRooms']for o in values),finalOnlineRoomCount=summarize_balance.stats(o['finalOnlineRooms']for o in values))
    report=dict(recordedAtUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),scope='All1000 final Saves and accepted ordinary build-order histories read only. Accepted order time is not proof of room completion/online time; no complete control/loss time-series attribution is invented.',branchesExcludingSelf=groups,selectedFour=[o for o in observations if o['index']in[760,921,780,941]],observations=observations,limits=['Distance is from the first accepted research-lab center, explicitly not substituted for the bot exact base-distance predicate.','Room inventories, powered/online and alive status describe final state only.','Final-room advantage correlated with outcomes is not itself a controlled causal intervention.'])
    dest=OUT/'energy-defense-observations.json'
    with dest.open('x',encoding='utf-8')as f:json.dump(report,f,ensure_ascii=False,indent=2)
    print(json.dumps(dict(report=str(dest),groups=groups),ensure_ascii=True),flush=True)


if __name__=='__main__':main()
