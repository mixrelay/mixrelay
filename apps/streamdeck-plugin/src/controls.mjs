import { randomUUID } from 'node:crypto';
import { isAbsolute, join } from 'node:path';
import { operations } from './capabilities.mjs';

export function targetNode(snapshot, settings) {
  return snapshot?.graphs
    ?.find((graph) => graph.id === settings.graphId)
    ?.nodes.find((node) => node.nodeId === settings.nodeId);
}

function requireRunning(snapshot, node) {
  if (snapshot.enginePower && (!snapshot.enginePower.enabled || snapshot.enginePower.transitioning))
    throw new Error('Audio engine is off or changing');
  if (!node.control.enabled) throw new Error('Node is off');
}

/** Derive an absolute, revision-fenced command from the state shown on the device. */
export function commandFor(snapshot, settings, operation = settings.operation, ticks = 1) {
  const node = targetNode(snapshot, settings);
  if (
    !snapshot?.session ||
    !Number.isSafeInteger(snapshot.revision) ||
    !operations(node).includes(operation)
  )
    throw new Error('Target unavailable');
  const base = {
    session: snapshot.session,
    target: { graphId: settings.graphId, nodeId: settings.nodeId },
  };
  const request = { expectedRevision: snapshot.revision };
  if (operation === 'power') request.enabled = !node.control.enabled;
  else {
    requireRunning(snapshot, node);
    if (operation === 'mute') request.muted = !node.control.muted;
    else if (operation.startsWith('gain_')) {
      if (!Number.isFinite(ticks)) throw new Error('Invalid dial value');
      request.gainDb = Math.max(
        -60,
        Math.min(12, node.gainDb + ticks * (operation === 'gain_up' ? 0.5 : -0.5)),
      );
    } else return recorderCommand(base, request, node, settings, operation);
  }
  return { type: 'node', ...base, request };
}

function recorderCommand(base, request, node, settings, action) {
  const recording = node.recorder;
  if (!recording) throw new Error('Recorder unavailable');
  if (
    (action === 'record' && recording.state === 'recording') ||
    (action === 'pause' && recording.state !== 'recording') ||
    (action === 'stop' && !['recording', 'paused'].includes(recording.state))
  )
    throw new Error('Recording state changed');
  const transport = { action };
  if (action === 'record' && recording.state !== 'paused') {
    if (typeof settings.recordingFolder !== 'string' || !isAbsolute(settings.recordingFolder))
      throw new Error('Choose an absolute recording folder');
    transport.path = join(
      settings.recordingFolder,
      `MixRelay-${new Date().toISOString().replaceAll(':', '-')}-${randomUUID()}.wav`,
    );
    // Follow input sample rate; use the same PCM24 default as the app.
  }
  return {
    type: 'recorder',
    ...base,
    request: {
      ...request,
      request: transport,
      expectedRecording: { state: recording.state, path: recording.path ?? null },
    },
  };
}

export function presentation(snapshot, settings) {
  const node = targetNode(snapshot, settings);
  if (!snapshot) return { title: 'MixRelay', value: 'Disconnected' };
  if (!node || !operations(node).includes(settings.operation))
    return { title: 'MixRelay', value: 'Unavailable' };
  const title = node.label || node.nodeId;
  if (!node.control.enabled) return { title, value: 'OFF' };
  if (snapshot.enginePower && (!snapshot.enginePower.enabled || snapshot.enginePower.transitioning))
    return { title, value: 'Engine OFF' };
  if (settings.operation === 'mute')
    return { title, value: node.control.muted ? 'MUTED' : 'Unmuted' };
  if (settings.operation?.startsWith('gain_'))
    return { title, value: `${node.gainDb.toFixed(1)} dB` };
  if (['record', 'pause', 'stop'].includes(settings.operation))
    return { title, value: node.recorder?.state ?? 'Unavailable' };
  return { title, value: 'ON' };
}
