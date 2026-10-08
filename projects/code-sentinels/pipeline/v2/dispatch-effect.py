"""Dispatch real effect production through the RurixForge Codex engine."""
import argparse
import json
import sys
from pathlib import Path

HERE=Path(__file__).resolve().parent
sys.path.insert(0,str(HERE.parent/'python-libs'))
import requests

parser=argparse.ArgumentParser()
parser.add_argument('effect',choices=['deepseek-tide','gpt-nova','pycharm-matrix'])
args=parser.parse_args()
effect=args.effect
api='http://127.0.0.1:8103/api/forge'
sessions={'deepseek-tide':'sess_1788670123057_5380645c','gpt-nova':'sess_1788670201477_0bd1ccb8'}
saved=HERE/f'{effect}.codex-session.json'
if saved.exists():
    sid=json.loads(saved.read_text(encoding='utf-8'))['session']['id']
elif effect in sessions:
    sid=sessions[effect]
    data=requests.get(f'{api}/sessions/{sid}',timeout=15).json()
    saved.write_text(json.dumps(data,ensure_ascii=False,indent=2),encoding='utf-8')
else:
    response=requests.post(f'{api}/sessions',json={'title':'编译防线V2 · PyCharm调试矩阵真实特效','agentKind':'coding','agentEngine':'codex','workspaceId':'ws_1788669812422_35134176'},timeout=20)
    response.raise_for_status()
    data=response.json()
    sid=data['session']['id']
    saved.write_text(json.dumps(data,ensure_ascii=False,indent=2),encoding='utf-8')
prompt=f'''用户已授权V2高算力技能的真实特效视频动画。你是本项目Codex引擎媒体agent，本轮只制作{effect}。请立即在D:/RurixForge/projects/code-sentinels执行 ./pipeline/v2/generate-effect.ps1 -Effect {effect} 。该项目脚本已准备真实MiniMax-H3图生视频的私有首帧与动作prompt，全程纯黑背景、固定镜头中心比例、旋转内聚集、短时爆发、最终消散，不产生环境。只提交一次已授权真实6秒I2V，并等待完成，通常数分钟。之后服务端ffmpeg解码完整72帧@12fps，以新black模式将黑底转换成保留彩色辉光的透明度，crop=none允许消散空帧。最终48帧GPU图集由主任务根据真实源帧等时采样打包，你只需完成脚本并验证实际视频mode=image2video、文件存在、截帧回执72帧。不得修改脚本、参数、游戏、前端或其他特效文件，不得以静态/程序特效替代，不得删除attempt盲重试。失败请报告供应商实际任务ID和错误；成功请报告任务ID、永久视频路径和原始alpha图集路径。'''
(HERE/f'{effect}.codex-prompt.txt').write_text(prompt,encoding='utf-8')
print(f'Dispatched {effect} to project Codex session {sid}',flush=True)
response=requests.post(f'{api}/sessions/{sid}/ask:execute',json={'userInput':prompt,'mode':'build','includeLibrary':False},timeout=1800)
response.raise_for_status()
result=response.json()
(HERE/f'{effect}.codex-result.json').write_text(json.dumps(result,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps(result,ensure_ascii=True),flush=True)
