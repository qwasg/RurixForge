"""Local isolated native engine MCP transport used for regression evidence."""
import os,json,pathlib,subprocess
ROOT=pathlib.Path(__file__).resolve().parents[1]
REPO=ROOT.parents[1]
class Mcp:
    def __init__(self,binary='engine-scene-mcp',extra_env=None):
        env=dict(os.environ,FORGE_PROJECT_ROOT=str(ROOT),FORGE_CODE_FORGE_PROJECT=str(ROOT),FORGE_RURIXC='D:/Rurix/target/debug/rurixc.exe')
        if extra_env:env.update(extra_env)
        log=ROOT/'game/native'/f'{binary}.stderr.log';log.parent.mkdir(parents=True,exist_ok=True)
        self.err=log.open('w',encoding='utf8')
        self.proc=subprocess.Popen([str(REPO/'target/debug'/f'{binary}.exe')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.err,env=env,text=True,encoding='utf8')
        self.id=0
        self.request('initialize',{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'code-sentinels-regression','version':'1'}})
        self.send({'jsonrpc':'2.0','method':'notifications/initialized','params':{}})
    def send(self,msg):self.proc.stdin.write(json.dumps(msg)+'\n');self.proc.stdin.flush()
    def request(self,method,params):
        self.id+=1;self.send({'jsonrpc':'2.0','id':self.id,'method':method,'params':params})
        while True:
            line=self.proc.stdout.readline()
            if not line:raise RuntimeError('Native MCP closed; inspect stderr evidence')
            value=json.loads(line)
            if value.get('id')==self.id:return value
    def call(self,name,args=None):
        response=self.request('tools/call',{'name':name,'arguments':args or {}})
        if 'error' in response:raise RuntimeError(response['error'])
        result=response.get('result',{});content=result.get('content',[])
        if result.get('isError'):raise RuntimeError(result)
        if content and content[0].get('type')=='text':
            try:return json.loads(content[0]['text'])
            except ValueError:return {'text':content[0]['text']}
        return result
    def close(self):
        # Closing stdin asks the supervisor to cleanly reap its owned host.
        self.proc.stdin.close()
        try:self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:self.proc.terminate()
        self.err.close()
if __name__=='__main__':
    import sys
    m=Mcp()
    try:print(json.dumps(m.call(sys.argv[1],json.loads(sys.argv[2]) if len(sys.argv)>2 else {}),ensure_ascii=False))
    finally:m.close()
