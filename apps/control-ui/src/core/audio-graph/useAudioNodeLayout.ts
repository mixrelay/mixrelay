import { useResizeObserver } from '@mantine/hooks';
import { useUpdateNodeInternals } from '@xyflow/react';
import { useEffect, useState } from 'react';

import { audioNodeFaceGeometry } from '../ui/audioNodeLayout';
import type { AudioGraphNode } from './model';

export function graphNodeFaceMode(node: AudioGraphNode) {
  if (node.runtime?.type === 'provider') return 'compact';
  return node.runtime?.type === 'recorder' ? 'channel-rows' : 'standard';
}

export function graphNodeFaceGeometry(node: AudioGraphNode) {
  return audioNodeFaceGeometry({
    channels: node.appearance.meterChannels,
    hasDetail: Boolean(node.appearance.detail),
    portCount: Math.max(node.inputPorts.length, node.outputPorts.length),
    mode: graphNodeFaceMode(node),
  });
}

export function useAudioNodeLayout(
  node: AudioGraphNode,
  editor: { width: number | null; height: number | null },
) {
  const [ref, bounds] = useResizeObserver<HTMLDivElement>();
  const [faceHeight, onFaceHeightChange] = useState(0);
  const [contentHeight, onContentHeightChange] = useState(0);
  const updateNodeInternals = useUpdateNodeInternals();
  const face = graphNodeFaceGeometry(node);
  const compact = graphNodeFaceMode(node) === 'compact';
  const portCount = Math.max(node.inputPorts.length, node.outputPorts.length);
  const minHeight = compact
    ? Math.max(88, 26 + (portCount + 1) * 18, face.height + contentHeight + 2)
    : face.height + contentHeight + 2;
  const nativeHeight =
    editor.width && editor.height
      ? (editor.height * (node.size?.width ?? Math.min(1600, Math.max(240, editor.width)))) /
          editor.width +
        28
      : 0;
  const ports = compact
    ? {
        ...face,
        height: Math.max(minHeight, node.size?.height ?? nativeHeight, bounds.height),
        portInsetTop: 26,
        portInsetBottom: 12,
      }
    : {
        ...face,
        height: faceHeight || face.height,
        firstChannelCenter:
          face.firstChannelCenter === null
            ? null
            : face.firstChannelCenter + Math.max(0, faceHeight - face.height) / 2,
      };
  const signature = [...node.inputPorts, ...node.outputPorts].map((port) => port.id).join('|');
  useEffect(() => {
    void signature;
    void editor.width;
    void ports.height;
    void ports.firstChannelCenter;
    updateNodeInternals(node.id);
  }, [
    node.id,
    signature,
    editor.width,
    ports.height,
    ports.firstChannelCenter,
    updateNodeInternals,
  ]);
  return { ref, face, ports, minHeight, onFaceHeightChange, onContentHeightChange };
}
