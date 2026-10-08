"""Normal-command economy, power, research and three-level campaign scenarios."""
import math
from native_harness import xy, footprint


def setup_economy(h, large=False):
    # Deposits, infrastructure and upgrades are purchased at real native prices.
    h.buy(9)
    h.fund(3000)
    for _ in range(4): h.buy(9)
    h.fund(12000 if large else 6000)
    dc = h.buy(2)
    dc_cell = round(h.building(dc)[1])
    winds = [h.buy(3, near=dc_cell) for _ in range(3)]
    power_links = [h.connect(1, round(h.building(w)[1]), dc_cell) for w in winds]
    for bay in range(2): h.command(3_000_000 + dc * 100 + bay * 10 + 1)
    lab = h.buy(8, near=dc_cell)
    lab_cell = round(h.building(lab)[1])
    h.connect(1, dc_cell, lab_cell)
    h.connect(2, dc_cell, lab_cell)
    h.advance(2.2)
    assert h.get(8) == 24 and h.get(9) == 600 and h.building(lab)[5] == 1
    h.record("power_and_compute_production", dc=dc, dcCell=dc_cell, power=h.fields(6,5), dcState=h.building(dc))
    return dc, lab, winds, power_links


def economy(h):
    h.reset()
    dc, lab, winds, power_links = setup_economy(h)
    dc_cell, lab_cell = round(h.building(dc)[1]), round(h.building(lab)[1])
    h.advance(7)
    before = h.anim(dc)
    for link in power_links: h.command(5_800_000+link)
    h.advance(.1)
    stopped = h.anim(dc)
    assert stopped[0] == 3 and h.get(8) == 0
    h.advance(1)
    assert h.anim(dc)[2] == stopped[2]
    for wind in winds: h.connect(1, round(h.building(wind)[1]), dc_cell)
    h.advance(.1)
    assert h.anim(dc)[0] == 2 and h.anim(dc)[2] > stopped[2]
    h.record("power_loss_freezes_work_then_resumes", before=before, stopped=stopped, resumed=h.anim(dc))
    h.advance(10)
    assert h.anim(lab)[0] == 3
    balances = h.fields(0,2)
    h.command(9_000_001)
    assert h.get(11) == 1 and abs(h.get(0)-(balances[0]-250)) < .02 and abs(h.get(1)-(balances[1]-120)) < .02
    h.advance(.1)
    assert h.anim(lab)[0] == 2
    h.advance(6.1)
    assert h.anim(lab)[0] == 3
    h.record("research_spends_real_balances_and_animates", before=balances, after=h.fields(0,2), tech=h.get(11), lab=h.anim(lab))
    # Choose two free cells on one row, using a cable that crosses the first AI.
    tiles, occupied = h.tiles(), h.occupied()
    row = dc_cell // 32
    free = [row*32+x for x in range(dc_cell%32+2, 14)
            if row*32+x not in occupied and tiles[row*32+x] not in (1,2)]
    middle, endpoint = free[-3], free[-1]
    a, b = h.buy_unit(3,middle), h.buy_unit(3,endpoint)
    calc = h.connect(2,dc_cell,endpoint)
    assert h.unit(a)[7] == 0 and h.unit(b)[7] == 1
    move_to = h.candidates(3,near=endpoint)[0]
    assert h.command(4_300_000+b*1000+move_to,required=False) == 0 and h.get(14) == 42
    # Removing all actual power generation leaves the cable tether intact.
    for wind in winds: h.command(2_100_000+wind)
    assert h.unit(b)[7] == 1 and h.unit(b)[6] == 0
    assert h.command(4_300_000+b*1000+move_to,required=False) == 0
    h.command(5_800_000+calc)
    assert h.unit(b)[7] == 0
    h.command(4_300_000+b*1000+move_to)
    h.advance(.25)
    assert h.unit(b)[18:20] != list(xy(endpoint))
    h.record("endpoint_only_tether_survives_power_loss", middle=h.unit(a), released=h.unit(b))


def fortify(h):
    dc, lab, winds, _ = setup_economy(h,large=True)
    dc_cell = round(h.building(dc)[1])
    while h.get(11) < 3:
        h.advance(20)
        h.command(9_000_001)
    h.fund(30000)
    nuclear = h.buy(6)
    h.command(2_000_000+nuclear); h.command(2_000_000+nuclear)
    h.connect(1,round(h.building(nuclear)[1]),dc_cell)
    for i in range(32):
        if h.get(2000+i*12+5): h.command(3_200_000+i)
    for bay in range(4):
        h.command(3_000_000+dc*100+bay*10+7)
    for i in range(32):
        if h.get(2000+i*12+5):
            h.command(3_100_000+i); h.command(3_100_000+i)
    relays = []
    for near in (8+8*32,8+12*32): relays.append(h.buy(7,near=near))
    tiles = h.tiles()
    # Defenders stand on the three real approaches, keeping each cell purchase legal.
    slots = []
    for _ in range(28):
        candidates = h.candidates(3,near=8+10*32,predicate=lambda c: 5<=c%32<=12 and 5<=c//32<=15)
        if not candidates: break
        def score(c):
            rows=min(abs(c//32-5),abs(c//32-10),abs(c//32-15))
            return rows*2+abs(c%32-9)-(2 if tiles[c]==4 else 0)
        c = min(candidates,key=score)
        slot = h.buy_unit(2 if len(slots)%3 else 1,c)
        h.command(4_100_000+slot);h.command(4_100_000+slot)
        slots.append(slot)
    h.advance(8)
    assert h.get(8)>1000 and all(h.unit(i)[6] for i in slots)
    h.record("campaign_defense_bought",level=h.get(5),tech=h.get(11),production=h.get(8),credits=h.get(0),defenders=len(slots),nuclear=h.building(nuclear))
    return slots


def campaign(h):
    h.reset()
    maps=[]
    for level in range(1,4):
        if level>1: h.command(10_100_000)
        assert h.get(5)==level
        initial_tiles=h.tiles()
        slots=fortify(h)
        observed_attack_frames=set()
        attacks_start=sum(h.unit(i)[17] for i in slots)
        damaged=False
        for wave in range(1,5):
            h.command(10_000_000)
            for batch in range(1600):
                h.advance(.25)
                for i in slots:
                    u=h.unit(i)
                    if u[0]>0 and u[17]>0:
                        observed_attack_frames.add(round(h.anim(i,True)[1]))
                    if u[0]>0 and u[3]<250: damaged=True
                if h.get(3) != 1: break
            h.record("campaign_wave",level=level,wave=wave,phase=h.get(3),coreHp=h.get(2),enemies=h.get(15),credits=h.get(0),compute=h.get(1),attacks=sum(h.unit(i)[17] for i in slots),frames=sorted(observed_attack_frames))
            assert h.get(3) != 3, f"defeat at level {level} wave {wave}"
            assert h.get(3) == (2 if wave==4 else 0), f"wave did not complete at level {level} wave {wave}"
        assert len(observed_attack_frames)>12 and sum(h.unit(i)[17] for i in slots)>attacks_start
        maps.append({"level":level,"changedCells":sum(a!=b for a,b in zip(initial_tiles,h.tiles())),"damagedDefenders":damaged})
        assert maps[-1]["changedCells"]>0
        frozen=h.fields(0,12)
        time_before=h.get(12000)
        h.advance(5)
        assert frozen==h.fields(0,12) and h.get(12000)>time_before+4.5
        h.record("victory_freezes_economy_visual_clock_continues",level=level,economy=frozen,clockBefore=time_before,clockAfter=h.get(12000),unlocked=h.get(24))
    assert h.get(24)==3
    h.record("three_level_campaign_completed",maps=maps)


def battle(h):
    h.reset()
    dc, _, _, _ = setup_economy(h)
    h.advance(10);h.command(9_000_001)
    h.buy(7,near=326)
    cell=h.candidates(3,near=327)[0]
    unit=h.buy_unit(1,cell)
    h.advance(3)
    assert h.anim(unit,True)[0]==3 and h.unit(unit)[6]==1
    h.command(10_000_000)
    for _ in range(1000):
        h.advance(.05)
        if h.unit(unit)[17]>0:break
    assert h.unit(unit)[17]>0 and h.anim(unit,True)[0]==2
    attacking=h.anim(unit,True)
    h.advance(.5,.05)
    assert h.anim(unit,True)[2]>attacking[2]+.4
    h.record("paid_attack_starts_and_advances_work",unit=unit,first=attacking,later=h.anim(unit,True))
    for i in range(32):
        if h.get(2000+i*12+5):h.command(3_200_000+i)
    h.advance(.05)
    stopped=h.anim(unit,True)
    assert stopped[0]==3 and h.unit(unit)[6]==0
    h.advance(.5,.05)
    assert h.anim(unit,True)[2]==stopped[2]
    h.record("compute_supply_loss_stops_software_work",unit=h.unit(unit),animation=h.anim(unit,True))
    sequence=round(h.get(12001));observed=[];hit=None;death=None;repaired=False
    for _ in range(6000):
        h.advance(.05)
        events=h.events_since(sequence)
        if events:
            sequence=round(events[-1][0])
            observed.extend(e for e in events if e[1] in (4,5,6,11,13,17))
        u=h.unit(unit)
        if hit is None and 0<u[3]<150:
            hit={"unit":u,"animation":h.anim(unit,True)}
        if death is None and h.anim(unit,True)[0]==4:
            death={"unit":u,"animation":h.anim(unit,True)}
        if not repaired and 0<h.get(2)<400:
            hp=h.get(2);credits=h.get(0)
            h.command(2_200_000)
            assert h.get(2)>hp and abs(h.get(0)-(credits-40))<.02
            h.record("repair_after_real_enemy_hit",beforeHp=hp,afterHp=h.get(2),creditsSpent=credits-h.get(0))
            repaired=True
        if h.get(3)==3:break
    assert hit and death and repaired
    assert any(e[1]==6 for e in observed) and any(e[1]==17 for e in observed)
    assert h.get(3)==3
    h.record("real_unit_damage_and_death",hit=hit,death=death,events=observed[:18])
    initial=h.anim(0);economy=h.fields(0,12);clock=h.get(12000)
    assert initial[0]==4 and h.get(2)==0
    h.advance(1.85,.05);middle=h.anim(0)
    assert middle[0]==4
    h.advance(.3,.05);wreck=h.anim(0)
    assert wreck[0:2]==[5,127]
    h.command(10_000_002);paused=h.anim(0)+h.fields(12000,6)
    h.advance(1)
    assert paused==h.anim(0)+h.fields(12000,6)
    h.command(10_000_002)
    h.advance(2.2,.05)
    assert h.anim(0)[0]==5
    h.advance(.25,.05)
    assert h.anim(0)[0]==0 and economy==h.fields(0,12)
    h.record("defeat_core_tail_continues_economy_stops",initial=initial,middle=middle,wreck=wreck,final=h.anim(0),clockBefore=clock,clockAfter=h.get(12000),economy=economy)


def walls(h):
    h.reset()
    dc,_,winds,_=setup_economy(h)
    h.fund(9000)
    # A second powered compute center is enclosed by ordinary wall commands.
    tiles,occupied=h.tiles(),h.occupied()
    center=None
    for c in h.candidates(2,near=137):
        row,col=divmod(c,32)
        if not (1<=col<=11 and 1<=row<=16):continue
        bounds=(col-1,row-1,col+2,row+2)
        x0,y0,x1,y1=bounds
        edge={r*32+x for r in range(y0,y1+1) for x in range(x0,x1+1)
              if r in (y0,y1) or x in (x0,x1)}
        if all(tiles[e] not in (1,2) and e not in occupied for e in edge):center=c;break
    assert center is not None
    dc2=h.buy(2,center)
    h.connect(1,round(h.building(winds[0])[1]),center)
    h.command(3_000_000+dc2*100+1)
    sequence=round(h.get(12001))
    x0,y0,x1,y1=bounds
    for a,b in [(y0*32+x0,y0*32+x1),(y1*32+x0,y1*32+x1),
                (y0*32+x0,y1*32+x0),(y0*32+x1,y1*32+x1)]:
        h.command(7_000_000+a*1000+b)
    events=h.events_since(sequence)
    assert len([e for e in events if e[1]==15])==len(edge)
    h.advance(4)
    assert h.get(19)>0 and h.get(20)>0
    h.record("closed_wall_region_charges_shield",center=center,edgeCells=len(edge),charge=h.get(19),capacity=h.get(20),landEvents=events)
    h.command(10_300_000)
    charge=h.get(19);h.advance(1)
    assert h.get(19)==charge
    h.command(10_300_000)
    # Removing one segment must publish complete destruction and open the region.
    wall=next(iter(edge));sequence=round(h.get(12001))
    h.command(7_800_000+wall)
    destroyed=h.events_since(sequence)
    event=next(e for e in destroyed if e[1]==16)
    assert event[6]==4.5 and event[7]==wall and h.get(20)==0
    h.record("wall_destroy_opens_shield_region",event=event,capacity=h.get(20))
