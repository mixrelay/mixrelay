import { expect, it } from 'vitest';

import { graphHistoryGroup } from './graphHistoryGroup';
import { addRoutingNode, runtimeGraphToAudioGraph, updateRoutingNodeParameter } from './model';

it('gives each control on one compressor its own history group', () => {
  const before = addRoutingNode(
    runtimeGraphToAudioGraph({ schemaVersion: 2, revision: 1, nodes: [], edges: [] }),
    { kind: 'compressor' },
    { x: 0, y: 0 },
  );
  const id = before.nodes[0]?.id ?? '';
  const threshold = updateRoutingNodeParameter(before, id, 'thresholdDb', -12);
  const ratio = updateRoutingNodeParameter(threshold, id, 'ratio', 6);
  const thresholdGroup = graphHistoryGroup(before, threshold, 'level');
  const ratioGroup = graphHistoryGroup(threshold, ratio, 'level');
  expect(thresholdGroup).toBeDefined();
  expect(ratioGroup).toBeDefined();
  expect(thresholdGroup).not.toBe(ratioGroup);
});
