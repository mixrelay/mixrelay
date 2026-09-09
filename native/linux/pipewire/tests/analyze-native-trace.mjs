// Lab-only boundary correlation; timestamps are monotonic nanoseconds.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
const directory = process.argv[2];
const warmup = Number(process.argv[3] ?? 40);
const sender = JSON.parse(readFileSync(join(directory, 'sender.json'), 'utf8'));
const lines = readFileSync(join(directory, 'native-trace.csv'), 'utf8').trim().split('\n');
const headers = lines.shift().split(',');
const rows = lines.map(line => Object.fromEntries(line.split(',').map((value, i) => [headers[i], i ? Number(value) : value])));
const start = sender.firstSendNs + warmup * 1e9;
const end = sender.lastSendNs - 20e6;
const banks = Object.fromEntries(['producer', 'consumer'].map(kind => [kind, rows.filter(row => row.kind === kind)]));
for (const bank of Object.values(banks)) for (let i = 1; i < bank.length; i++) {
  const row = bank[i], previous = bank[i - 1];
  row.actualDeltaMs = (row.actualNs - previous.actualNs) / 1e6;
  row.positionDelta = row.position - previous.position;
  row.clockDeltaMs = (row.clockNs - previous.clockNs) / 1e6;
  row.callbackLatenessMs = (row.actualNs - row.clockNs) / 1e6;
}
const measured = bank => bank.filter(row => row.actualNs >= start && row.actualNs <= end);
const summarize = bank => {
  const selected = measured(bank);
  return {
    callbacks: selected.length,
    frames: selected.reduce((n, row) => n + row.frames, 0),
    returnedFrames: selected.reduce((n, row) => n + row.returned, 0),
    missingPositionFrames: selected.reduce((n, row) => n + Math.max(0, row.positionDelta - row.frames), 0),
    repeatedPositions: selected.filter(row => row.positionDelta === 0).length,
    maxCallbackIntervalMs: Math.max(...selected.map(row => row.actualDeltaMs)),
    maxCallbackLatenessMs: Math.max(...selected.map(row => row.callbackLatenessMs)),
    zeroReturns: selected.filter(row => row.returned === 0).length,
    stereoMismatches: selected.reduce((n, row) => n + (row.stereoMismatches ?? 0), 0),
    stereoMismatchBlocks: selected.filter(row => row.stereoMismatches > 0),
    discontinuities: selected.filter(row => row.positionDelta !== row.frames || row.actualDeltaMs > Math.max(10, row.frames / 48 * 2)),
  };
};
const gaps = measured(banks.consumer).filter(row => !row.returned).map(row => {
  const nearby = rows.filter(candidate => Math.abs(candidate.actualNs - row.actualNs) < Math.max(20e6, row.frames / 48000 * 4e9))
    .sort((a, b) => a.actualNs - b.actualNs);
  return { ...row, secondsFromSender: (row.actualNs - sender.firstSendNs) / 1e9, nearby };
});
const pcmMismatches = measured(banks.consumer).filter(row => row.stereoMismatches > 0).map(row => ({
  consumer: row,
  producers: banks.producer.filter(candidate => candidate.written < row.read + row.returned && candidate.written + candidate.returned > row.read),
}));
console.log(JSON.stringify({ directory, warmup, sender, producer: summarize(banks.producer), consumer: summarize(banks.consumer), gaps, pcmMismatches }, null, 2));
