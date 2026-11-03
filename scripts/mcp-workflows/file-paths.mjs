// One-pass agent defaults: completion, bounded pages and cleanup are server-owned.
export default async ({call})=>{
  const read={projection:['path'],distinct_paths:true};
  let result=await call('winproc_start_capture',{kind:'file_users',read});
  while(result.data.state==='running'){
    const next=result.data.next_action;
    if(!next)throw Error('Running job has no continuation');
    result=await call(next.tool,next.arguments);
  }
  if(result.data.state!=='succeeded')throw Error(JSON.stringify(result.data.result));
  const first=result.data.result_page;
  if(!first||first.error)throw Error(JSON.stringify(first??'Missing result page'));
  const paths=[];let page=first;
  while(true){
    paths.push(...page.data.rows.map(row=>row.path));
    const next=page.data.next_action;
    if(!next)break;
    page=await call(next.tool,next.arguments);
  }
  if(paths.length!==first.data.coverage.listed||page.data.capture_retained!==false)throw Error('Incomplete managed result');
  return {status:first.status,coverage:first.data.coverage,paths};
};
