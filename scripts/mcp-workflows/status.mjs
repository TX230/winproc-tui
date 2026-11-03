export default async ({call})=>{
  const response=await call('winproc_get_status');
  return {activity:response.data.activity,stale:response.data.stale};
};
