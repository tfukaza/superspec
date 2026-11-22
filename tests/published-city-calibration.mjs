// Each label appears twice at each position, and every ordered pair of labels
// occurs equally often within the six rounds. Controls run identical code.
const order = [
  ["control", "repeat", "timed"], ["timed", "repeat", "control"],
  ["repeat", "timed", "control"], ["control", "timed", "repeat"],
  ["timed", "control", "repeat"], ["repeat", "control", "timed"],
];

function difference(reference, trial) {
  return {
    meanFrameMs: trial.frameMs.mean - reference.frameMs.mean,
    medianCpuMs: trial.cpuMs.median - reference.cpuMs.median,
    framesOver25ms: trial.framesOver25ms - reference.framesOver25ms,
    posesWithDifferentWork: trial.samples.filter((sample, i) => sample.calls !== reference.samples[i].calls
      || sample.triangles !== reference.samples[i].triangles).length,
  };
}

export async function calibrateView(name, gpuSampleEvery, measureRoute) {
  // Exercise shader, texture and visibility paths along the whole trajectory
  // before measuring. Measurement order and both warmup runs stay explicit.
  await measureRoute(0);
  const primer = await measureRoute(gpuSampleEvery);
  if (!primer.gpuTimerAvailable) throw Error("GPU timer queries are unavailable for this calibration.");
  if (!primer.gpuSamples) throw Error("The instrumented warmup returned no valid GPU timings.");
  const rounds = [];
  for (const sequence of order) {
    const runs = {};
    for (const label of sequence) runs[label] = await measureRoute(label === "timed" ? gpuSampleEvery : 0);
    rounds.push({ sequence, runs, repeatMinusControl: difference(runs.control, runs.repeat),
      timedMinusControl: difference(runs.control, runs.timed) });
  }
  const summarize = (key, field) => {
    const values = rounds.map(round => round[key][field]).sort((a, b) => a - b);
    return { min: values[0], median: (values[2] + values[3]) / 2, max: values.at(-1) };
  };
  const totals = Object.fromEntries(["control", "repeat", "timed"].map(label => {
    const trials = rounds.map(r => r.runs[label]);
    const frames = trials.reduce((n, t) => n + t.frames, 0);
    return [label, { frames, deliveredFps: 1000 * frames / trials.reduce((n, t) => n + t.frameMs.mean * t.frames, 0),
      framesOver25ms: trials.reduce((n, t) => n + t.framesOver25ms, 0),
      scheduledGpuSamples: trials.reduce((n, t) => n + t.scheduledGpuSamples, 0),
      disjointGpuSamples: trials.reduce((n, t) => n + t.disjointSamples, 0),
      unresolvedGpuSamples: trials.reduce((n, t) => n + t.unresolvedGpuSamples, 0),
      gpuSamples: trials.reduce((n, t) => n + t.gpuSamples, 0) }];
  }));
  const incompleteGpuRounds = rounds.flatMap(({ runs: { timed } }, i) => timed.gpuSamples === timed.scheduledGpuSamples
    ? [] : [{ round: i + 1, scheduled: timed.scheduledGpuSamples, valid: timed.gpuSamples,
      disjoint: timed.disjointSamples, unresolved: timed.unresolvedGpuSamples }]);
  return { name, width: primer.width, height: primer.height, gpu: primer.gpu, gpuSampleEvery, totals,
    incompleteGpuRounds,
    pairedRoundDifferences: Object.fromEntries(["repeatMinusControl", "timedMinusControl"].map(key =>
      [key, Object.fromEntries(Object.keys(rounds[0][key]).map(field => [field, summarize(key, field)]))])),
    rounds };
}
