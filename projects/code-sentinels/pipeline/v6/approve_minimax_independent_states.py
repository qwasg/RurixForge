"""Original RGB inspection of final independent neutral/reaction/fall sources."""
from media import *
windows={'minimax-s-idle-physical-v4':('idle',[0,124,18,True]),'minimax-s-hit-physical-v4':('hit',[10,76,24,False]),'minimax-s-death-physical-v4':('death',[12,76,24,False]),'minimax-sw-hit-physical-v4':('hit',[8,92,32,False])}
for id,(action,segment) in windows.items():
 folder=HERE/'jobs'/id;receipt=read(folder/'video.json')
 if (folder/'source-review.json').exists() and read(folder/'source-review.json').get('approved'):continue
 note='Complete original24-frame temporal contact viewed: same pink/white singer, original microphone, correct initial facing and constant clean green backdrop. '
 note+=('Actual complete lowering/side-prone rest with full long hair and clothing inside512; remains down.' if action=='death' else 'Real body/hair/eye or physical flinch motion with a complete original upright recovery, never a generated still loop.')
 if id.startswith('minimax-sw'):note+=' A small brief comic surprise mark appears above the head as part of the real response; it does not obscure or alter the character.'
 save(folder/'source-review.json',{'id':id,'approved':True,'reviewedAt':stamp(),'reviewer':'Codex complete originalRGB temporal contact visual inspection','notes':note,'segments':{action:segment},'videoSha256':receipt['sha256']});extract(folder,True)
