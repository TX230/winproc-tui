// One request to the existing live sampler; no new model session or forced sampling.
export default async ({call}) => {
  const result = await call('winproc_get_metrics', {metric_ids: ['cpu_average_percent']});
  if (result.source !== 'live') throw Error('Expected a live CPU sample');
  const sample = result.data.samples[0];
  const cpu = sample.values.cpu_average_percent;
  const age = Math.max(0, Date.now() - Date.parse(result.captured_at));
  return {
    ...(Number.isFinite(cpu) ? {cpu_percent: cpu} : {unavailable: sample.unavailable.cpu_average_percent ?? 'unknown'}),
    captured_at: result.captured_at, sample_age_ms: age, stale: age >= 3000,
    status: result.status, warnings: result.warnings,
  };
};
