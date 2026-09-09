// Analyze retained sender/production-native-probe/RTP DLL evidence only.
// Usage: node analyze-clock-drift.mjs --warmup=40 LAB_DIRECTORY [LAB_DIRECTORY ...]
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

const warmupArgument = process.argv.find((argument) => argument.startsWith('--warmup='));
const warmupSeconds = warmupArgument ? Number(warmupArgument.split('=')[1]) : 20;
if (!Number.isFinite(warmupSeconds) || warmupSeconds < 0) throw new Error('Invalid warmup');

function slope(samples, key) {
  if (samples.length < 2) return null;
  const origin = samples[0].time;
  const xs = samples.map((row) => row.time - origin);
  const meanX = xs.reduce((a, b) => a + b, 0) / xs.length;
  const meanY = samples.reduce((sum, row) => sum + row[key], 0) / samples.length;
  let covariance = 0;
  let variance = 0;
  for (let index = 0; index < samples.length; index++) {
    covariance += (xs[index] - meanX) * (samples[index][key] - meanY);
    variance += (xs[index] - meanX) ** 2;
  }
  return variance ? covariance / variance : null;
}

function summarize(directory) {
  const sender = JSON.parse(readFileSync(join(directory, 'sender.json'), 'utf8'));
  const probe = JSON.parse(readFileSync(join(directory, 'probe.json'), 'utf8'));
  const config = readFileSync(join(directory, 'pipewire.conf'), 'utf8');
  const log = readFileSync(join(directory, 'pipewire.log'), 'utf8').split('\n');
  const start = sender.firstSendNs / 1e9;
  const end = sender.lastSendNs / 1e9;
  const rows = [];
  const faults = [];
  for (const line of log) {
    const time = Number(line.match(/^\[[A-Z]\]\[\s*([\d.]+)\]/)?.[1]);
    if (!Number.isFinite(time) || time < start + warmupSeconds || time > end - 0.02) continue;
    const dll = line.match(/avail:(\d+) target:(\d+) error:([\d.-]+) corr:([\d.-]+)/);
    if (dll) rows.push({ time, avail: Number(dll[1]), target: Number(dll[2]), correction: Number(dll[4]) });
    if (/underrun|overrun|unexpected seq|sync to timestamp/.test(line)) faults.push(line);
  }
  const tail = rows.filter((row) => row.time >= end - 30);
  const average = (key) => tail.reduce((sum, row) => sum + row[key], 0) / tail.length;
  return {
    directory,
    compensationEnabled: !/resample.disable\s*=\s*true/.test(config),
    sender,
    probe,
    expectedClockCorrectedFrequency: 1000 * (1 + sender.observedPpm / 1e6),
    frequencyErrorHz: probe.observedFrequency - 1000 * (1 + sender.observedPpm / 1e6),
    warmupSeconds,
    analyzedSeconds: end - start - warmupSeconds,
    dllSamples: rows.length,
    tailSamples: tail.length,
    tailOccupancyMin: tail.length ? Math.min(...tail.map((row) => row.avail)) : null,
    tailOccupancyMax: tail.length ? Math.max(...tail.map((row) => row.avail)) : null,
    tailOccupancyMean: tail.length ? average('avail') : null,
    tailOccupancySlopeFramesPerSecond: slope(tail, 'avail'),
    tailCorrectionMean: tail.length ? average('correction') : null,
    tailDllCorrectionPpm: tail.length ? (average('correction') - 1) * 1e6 : null,
    tailRequestedResampleRatePpm: tail.length ? (1 / average('correction') - 1) * 1e6 : null,
    postWarmupFaultCount: faults.length,
    postWarmupFaults: faults,
  };
}

const directories = process.argv.slice(2).filter((argument) => !argument.startsWith('--warmup='));
if (!directories.length) throw new Error('Expected retained evidence directories');
console.log(JSON.stringify(directories.map(summarize), null, 2));
