import json,os,urllib.request
from pathlib import Path
home=Path(os.environ.get('CODEXY_HOME') or str(Path(os.environ.get('XDG_CONFIG_HOME',str(Path.home()/'.config')))/'codexy'))
auth=json.loads((home/'auth.json').read_text())
key='codexy-live-cache-validation-20260914'
text='Reference records:\n'+''.join(f'Record {i:04d}: This is stable synthetic reference text for a repeatable cache routing verification.\n' for i in range(240))+'\nReply with exactly OK.'
body={'model':'gpt-5.6-luna','input':[{'role':'user','content':[{'type':'input_text','text':text}]}],'prompt_cache_key':key,'store':False,'stream':True,'instructions':'','include':['reasoning.encrypted_content']}
for mode in ['direct','proxy','direct','proxy']:
    headers={'Content-Type':'application/json','session-id':key}
    if mode=='direct':
        headers.update({'Authorization':'Bearer '+auth['access_token'],'ChatGPT-Account-Id':auth['account_id']})
        url='https://chatgpt.com/backend-api/codex/responses'
    else:url='http://127.0.0.1:8787/v1/responses'
    req=urllib.request.Request(url,json.dumps(body,separators=(',',':')).encode(),headers)
    with urllib.request.urlopen(req,timeout=90) as response:
        usage=None
        for line in response:
            if not line.startswith(b'data:'):continue
            try:e=json.loads(line[5:])
            except ValueError:continue
            if e.get('type')=='response.completed':usage=e['response']['usage'];break
        assert usage is not None,'missing completion'
        print(json.dumps({'path':mode,'input_tokens':usage['input_tokens'],'cached_tokens':usage['input_tokens_details']['cached_tokens'],'output_tokens':usage['output_tokens']}),flush=True)
