import json, urllib.request, urllib.error
base='http://127.0.0.1:8787'
def request(path,payload):
    req=urllib.request.Request(base+path,json.dumps(payload).encode(),{'Content-Type':'application/json'})
    try:
        with urllib.request.urlopen(req,timeout=90) as response:
            if payload.get('stream'):
                events=[]; done=False
                for line in response:
                    if not line.startswith(b'data:'):continue
                    data=line[5:].strip()
                    if data==b'[DONE]':done=True;continue
                    events.append(json.loads(data))
                return events,done
            return json.load(response)
    except urllib.error.HTTPError as e:
        print('HTTP_ERROR',e.code,e.read().decode()[:800],flush=True)
        raise SystemExit(1)
text_input=[{'role':'user','content':[{'type':'input_text','text':'Reply with exactly OK.'}]}]
results=[]
for chat in [False,True]:
    path='/v1/chat/completions' if chat else '/v1/responses'
    for streaming in [False,True]:
        payload=({'messages':[{'role':'user','content':'Reply with exactly OK.'}]} if chat else {'input':text_input})
        payload['stream']=streaming
        if chat and streaming:payload['stream_options']={'include_usage':True}
        response=request(path,payload)
        if streaming:
            events,done=response
            if chat:
                text=''.join(c.get('delta',{}).get('content','') for e in events for c in e.get('choices',[]))
                assert done and any(e.get('usage') for e in events)
            else:
                text=''.join(e.get('delta','') for e in events if e.get('type')=='response.output_text.delta')
                assert any(e.get('type')=='response.completed' for e in events)
        elif chat:
            text=response['choices'][0]['message']['content']
            assert response['model']=='gpt-5.6-luna'
        else:
            text=''.join(p.get('text','') for i in response['output'] if i.get('type')=='message' for p in i.get('content',[]) if p.get('type')=='output_text')
            assert response['model']=='gpt-5.6-luna'
        assert text.strip()=='OK',(path,streaming,text)
        results.append(f'{path} text stream={streaming}')
        print('PASS',results[-1],flush=True)
    function={'name':'echo_number','description':'Return the supplied number','parameters':{'type':'object','properties':{'value':{'type':'integer'}},'required':['value']}}
    for streaming in [False,True]:
        if chat:
            payload={'messages':[{'role':'user','content':'Call echo_number with value 7.'}],'tools':[{'type':'function','function':function}],'tool_choice':{'type':'function','function':{'name':'echo_number'}},'stream':streaming}
        else:
            payload={'input':[{'role':'user','content':[{'type':'input_text','text':'Call echo_number with value 7.'}]}],'tools':[dict(type='function',strict=False,**function)],'tool_choice':{'type':'function','name':'echo_number'},'stream':streaming}
        response=request(path,payload)
        if streaming:
            events,done=response
            if chat:
                calls=[t for e in events for c in e.get('choices',[]) for t in c.get('delta',{}).get('tool_calls',[])]
                assert calls[0]['id'] and calls[0]['function']['name']=='echo_number'
                arguments=''.join(t['function'].get('arguments','') for t in calls)
                assert done and any(c.get('finish_reason')=='tool_calls' for e in events for c in e.get('choices',[]))
            else:
                calls=[e['item'] for e in events if e.get('type')=='response.output_item.done' and e.get('item',{}).get('type')=='function_call']
                assert calls[0]['call_id'] and calls[0]['name']=='echo_number'
                arguments=calls[0]['arguments']
        elif chat:
            c=response['choices'][0]
            assert c['finish_reason']=='tool_calls'
            call=c['message']['tool_calls'][0]
            assert call['id'] and call['function']['name']=='echo_number'
            arguments=call['function']['arguments']
        else:
            call=next(i for i in response['output'] if i['type']=='function_call')
            assert call['call_id'] and call['name']=='echo_number'
            arguments=call['arguments']
        assert json.loads(arguments)=={'value':7},arguments
        results.append(f'{path} function stream={streaming}')
        print('PASS',results[-1],flush=True)
print('LIVE CHECKS PASSED:',len(results),flush=True)
