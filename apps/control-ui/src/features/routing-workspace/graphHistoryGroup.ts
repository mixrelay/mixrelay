import type { AudioGraphChangeKind } from '../../core/audio-graph/AudioGraphCanvas';
import type { AudioGraphModel, AudioGraphNodeAppearance } from '../../core/audio-graph/model';

function changedControl(before: AudioGraphNodeAppearance, after: AudioGraphNodeAppearance) {
  const changed: string[] = [];
  if (before.gainControl?.valueDb !== after.gainControl?.valueDb) changed.push('gain');
  const previous = new Map(before.parameterControls?.map((control) => [control.id, control.value]));
  for (const control of after.parameterControls ?? []) {
    if (previous.get(control.id) !== control.value) changed.push(`parameter:${control.id}`);
  }
  return changed.length === 1 ? changed[0] : undefined;
}

export function graphHistoryGroup(
  before: AudioGraphModel,
  after: AudioGraphModel,
  kind: AudioGraphChangeKind,
) {
  if (
    before.nodes.length !== after.nodes.length ||
    before.nodes.some((node, index) => node.id !== after.nodes[index]?.id)
  ) {
    return undefined;
  }
  const changedIndices = before.nodes.flatMap((node, index) =>
    node !== after.nodes[index] ? [index] : [],
  );
  if (changedIndices.length !== 1 || before.edges !== after.edges) return undefined;
  const index = changedIndices[0] ?? -1;
  const previous = before.nodes[index];
  const next = after.nodes[index];
  if (!previous || !next) return undefined;
  const group = `${kind}:${next.id}`;
  if (kind !== 'level' && kind !== 'processor_parameter') return group;
  // One node can contain many controls. Threshold and ratio are separate gestures,
  // even when the second starts within the first control's debounce window.
  const control = changedControl(previous.appearance, next.appearance);
  return control ? `${group}:${control}` : undefined;
}
