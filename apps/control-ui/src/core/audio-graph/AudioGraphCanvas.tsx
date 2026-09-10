import '@xyflow/react/dist/base.css';

import { Tooltip } from '@mantine/core';
import {
  Background,
  BackgroundVariant,
  type Connection,
  type ConnectionLineComponentProps,
  ConnectionMode,
  type Edge,
  getBezierPath,
  Handle,
  type NodeChange,
  type NodeProps,
  NodeResizeControl,
  Position,
  ReactFlow,
  type ReactFlowInstance,
  Panel as ReactFlowPanel,
  type ReactFlowProps,
  useConnection,
  useEdgesState,
  useNodesState,
  useUpdateNodeInternals,
  useViewport,
  type XYPosition,
} from '@xyflow/react';
import {
  createContext,
  type Dispatch,
  memo,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  type PointerEvent as ReactPointerEvent,
  type SetStateAction,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from 'react';

import { AudioMeterProvider } from '../audio-meter/AudioMeterProvider';
import type { AudioMeterStore } from '../audio-meter/AudioMeterStore';
import { useAudioMeterLevels } from '../audio-meter/useAudioMeterLevels';
import type { ProcessorEditorFailure, ProcessorEditorStatus } from '../control-port/types';
import { AudioNodeCard } from '../ui/AudioNodeCard';
import { audioNodePortTop } from '../ui/audioNodeLayout';
import {
  HierarchicalContextMenu,
  HierarchicalMenuButton,
  type HierarchicalMenuItem,
} from '../ui/HierarchicalMenu';
import styles from './AudioGraphCanvas.module.css';
import {
  type AudioGraphConnection,
  type AudioGraphModel,
  type AudioGraphNode,
  type AudioGraphPort,
  canConnectAudioGraph,
  connectAudioGraphBatch,
  disconnectAudioGraph,
  moveAudioGraphNode,
  reconnectAudioGraph,
  removeAudioGraphItems,
  resizeAudioGraphNode,
} from './model';
import {
  type AudioFlowEdge,
  type AudioFlowNode,
  reconcileAudioFlowEdges,
  reconcileAudioFlowNodes,
} from './reactFlowAdapter';
import { graphNodeFaceGeometry, graphNodeFaceMode, useAudioNodeLayout } from './useAudioNodeLayout';
import { type GraphContextMenuLabels, useGraphContextMenu } from './useGraphContextMenu';

const nodeTypes = { audio: memo(AudioFlowNodeView) };
const deleteKeyCode = ['Backspace', 'Delete'];
const panActivationKeyCode =
  typeof navigator !== 'undefined' && /Macintosh|Mac OS|iPhone|iPad/u.test(navigator.userAgent)
    ? 'Meta'
    : 'Control';
const panMouseButtons = [1];
const snapGrid: [number, number] = [12, 12];
const portSelectionDistance = 3;
const portTooltipClassNames = { tooltip: styles.portTooltip };
const portTooltipTransition = { duration: 80 };
const emptyProcessorEditorStatuses = new Map<string, ProcessorEditorStatus>();
const NodeGainContext = createContext<((nodeId: string, gainDb: number) => void) | undefined>(
  undefined,
);
const NodeEndpointContext = createContext<
  | ((
      nodeId: string,
      value: { captureChannels: number; displayName: string; renderChannels: number },
    ) => void)
  | undefined
>(undefined);
const NodeParameterContext = createContext<
  ((nodeId: string, parameterId: string, value: number) => void) | undefined
>(undefined);
export interface ProcessorEditorLabels {
  retry: string;
  failure: Record<ProcessorEditorFailure, string>;
}
interface NodeProcessorEditorContextValue {
  labels: ProcessorEditorLabels;
  onRetry: (nodeId: string) => void;
  statuses: ReadonlyMap<string, ProcessorEditorStatus>;
}
const NodeProcessorEditorContext = createContext<NodeProcessorEditorContextValue | null>(null);
const NodeContentContext = createContext<((node: AudioGraphNode) => ReactNode) | undefined>(
  undefined,
);
export type AudioGraphChangeKind = 'layout' | 'level' | 'processor_parameter' | 'runtime';
export interface AudioGraphViewport {
  x: number;
  y: number;
  zoom: number;
}
type CommitGraph = (nextModel: AudioGraphModel, kind: AudioGraphChangeKind) => void;
type ReconnectEnd = NonNullable<ReactFlowProps<AudioFlowNode, AudioFlowEdge>['onReconnectEnd']>;

type AudioPortSide = 'input' | 'output';

interface SelectedAudioPort {
  nodeId: string;
  portId: string;
  side: AudioPortSide;
}

interface AudioPortSelectionContextValue {
  model: AudioGraphModel;
  selectedRef: { current: SelectedAudioPort[] };
}

const AudioPortSelectionContext = createContext<AudioPortSelectionContextValue | null>(null);

export interface AudioGraphCanvasProps {
  renderNodeContent?: (node: AudioGraphNode) => ReactNode;
  contextMenuLabels?: GraphContextMenuLabels;
  addNodeLabel?: string;
  ariaLabel: string;
  meterStore?: AudioMeterStore;
  model: AudioGraphModel;
  nodeMenuItems?: readonly AudioGraphNodeMenuItem[];
  onAddNode?: (itemId: string, position: XYPosition) => void;
  onNodeMenuOpenedChange?: (opened: boolean) => void;
  onModelChange: (model: AudioGraphModel, kind: AudioGraphChangeKind) => void;
  onNodeEndpointChange?: (
    nodeId: string,
    value: {
      captureChannels: number;
      displayName: string;
      renderChannels: number;
      sampleRate?: number;
    },
  ) => void;
  onNodeGainChange?: (nodeId: string, gainDb: number) => void;
  onNodeParameterChange?: (nodeId: string, parameterId: string, value: number) => void;
  onProcessorEditorRetry?: (nodeId: string) => void;
  processorEditorLabels?: ProcessorEditorLabels;
  processorEditorStatuses?: ReadonlyMap<string, ProcessorEditorStatus>;
  /** Initial or externally restored view; user movement remains locally interactive. */
  viewport?: AudioGraphViewport;
  /** Reports completed user pan/zoom gestures, never programmatic restoration. */
  onViewportChange?: (viewport: AudioGraphViewport) => void;
}

export type AudioGraphNodeMenuItem = HierarchicalMenuItem;

function AudioGraphProviders({
  children,
  editor,
  meterStore,
  model,
  onEndpointChange,
  onGainChange,
  onParameterChange,
  selectedRef,
  renderNodeContent,
}: {
  children: ReactNode;
  editor: NodeProcessorEditorContextValue | null;
  meterStore: AudioMeterStore | null;
  model: AudioGraphModel;
  onEndpointChange: AudioGraphCanvasProps['onNodeEndpointChange'];
  onGainChange: AudioGraphCanvasProps['onNodeGainChange'];
  onParameterChange: AudioGraphCanvasProps['onNodeParameterChange'];
  selectedRef: AudioPortSelectionContextValue['selectedRef'];
  renderNodeContent: AudioGraphCanvasProps['renderNodeContent'];
}) {
  return (
    <AudioMeterProvider store={meterStore}>
      <NodeProcessorEditorContext.Provider value={editor}>
        <NodeParameterContext.Provider value={onParameterChange}>
          <NodeEndpointContext.Provider value={onEndpointChange}>
            <NodeGainContext.Provider value={onGainChange}>
              <AudioPortSelectionContext.Provider value={{ model, selectedRef }}>
                <NodeContentContext.Provider value={renderNodeContent}>
                  {children}
                </NodeContentContext.Provider>
              </AudioPortSelectionContext.Provider>
            </NodeGainContext.Provider>
          </NodeEndpointContext.Provider>
        </NodeParameterContext.Provider>
      </NodeProcessorEditorContext.Provider>
    </AudioMeterProvider>
  );
}

function processorEditorContext(
  onRetry: AudioGraphCanvasProps['onProcessorEditorRetry'],
  labels: ProcessorEditorLabels | undefined,
  statuses: ReadonlyMap<string, ProcessorEditorStatus>,
): NodeProcessorEditorContextValue | null {
  return onRetry && labels ? { labels, onRetry, statuses } : null;
}

function useCanvasViewActions(
  onAddNode: AudioGraphCanvasProps['onAddNode'],
  onProcessorEditorRetry: AudioGraphCanvasProps['onProcessorEditorRetry'],
  processorEditorLabels: ProcessorEditorLabels | undefined,
  processorEditorStatuses: ReadonlyMap<string, ProcessorEditorStatus>,
  nodeMenu: ReturnType<typeof useNodeMenu>,
  clearPortSelection: () => void,
) {
  const addNode = useCallback(
    (itemId: string) => onAddNode?.(itemId, nodeMenu.positionRef.current),
    [nodeMenu.positionRef, onAddNode],
  );
  const onPaneClick = useCallback(() => {
    nodeMenu.close();
    clearPortSelection();
  }, [clearPortSelection, nodeMenu.close]);
  return {
    addNode,
    editorContext: processorEditorContext(
      onProcessorEditorRetry,
      processorEditorLabels,
      processorEditorStatuses,
    ),
    onPaneClick,
  };
}

function useNodeMenu() {
  const rootRef = useRef<HTMLElement | null>(null);
  const flowRef = useRef<ReactFlowInstance<AudioFlowNode, AudioFlowEdge> | null>(null);
  const positionRef = useRef<XYPosition>({ x: 0, y: 0 });
  const [buttonOpened, setButtonOpened] = useState(false);
  const [contextOpened, setContextOpened] = useState(false);
  const close = useCallback(() => {
    setButtonOpened(false);
    setContextOpened(false);
  }, []);
  const setFlowInstance = useCallback(
    (instance: ReactFlowInstance<AudioFlowNode, AudioFlowEdge>) => {
      flowRef.current = instance;
    },
    [],
  );
  const prepareContextPosition = useCallback((event: { clientX: number; clientY: number }) => {
    setButtonOpened(false);
    const bounds = rootRef.current?.getBoundingClientRect();
    const screenPosition = {
      x: event.clientX - (bounds?.left ?? 0),
      y: event.clientY - (bounds?.top ?? 0),
    };
    positionRef.current =
      flowRef.current?.screenToFlowPosition({ x: event.clientX, y: event.clientY }) ??
      screenPosition;
  }, []);
  const prepareButtonPosition = useCallback((event: ReactMouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    setContextOpened(false);
    const bounds = rootRef.current?.getBoundingClientRect();
    const screenPosition = { x: 132, y: 92 };
    positionRef.current =
      flowRef.current?.screenToFlowPosition({
        x: (bounds?.left ?? 0) + screenPosition.x,
        y: (bounds?.top ?? 0) + screenPosition.y,
      }) ?? screenPosition;
  }, []);
  return {
    buttonOpened,
    close,
    contextOpened,
    flowRef,
    positionRef,
    prepareButtonPosition,
    prepareContextPosition,
    rootRef,
    setButtonOpened,
    setContextOpened,
    setFlowInstance,
  };
}

function audioPortKey(port: SelectedAudioPort) {
  return `${port.side}\u0000${port.nodeId}\u0000${port.portId}`;
}

interface PortSelectionStart {
  additive: boolean;
  pointerId: number;
  x: number;
  y: number;
}

interface SelectionRectangle {
  bottom: number;
  left: number;
  right: number;
  top: number;
}

function portSelectionStart(event: ReactPointerEvent<HTMLElement>): PortSelectionStart | null {
  const target = event.target;
  if (
    event.pointerType !== 'mouse' ||
    event.button !== 0 ||
    !event.isPrimary ||
    event.metaKey ||
    event.ctrlKey ||
    !(target instanceof Element) ||
    !target.classList.contains('react-flow__pane')
  ) {
    return null;
  }
  return {
    additive: event.shiftKey,
    pointerId: event.pointerId,
    x: event.clientX,
    y: event.clientY,
  };
}

function selectionRectangle(
  start: PortSelectionStart,
  end: { x: number; y: number },
): SelectionRectangle {
  return {
    bottom: Math.max(start.y, end.y),
    left: Math.min(start.x, end.x),
    right: Math.max(start.x, end.x),
    top: Math.min(start.y, end.y),
  };
}

function pointInsideRectangle(point: DOMPoint, rectangle: SelectionRectangle) {
  return (
    point.x >= rectangle.left &&
    point.x <= rectangle.right &&
    point.y >= rectangle.top &&
    point.y <= rectangle.bottom
  );
}

function segmentsIntersect(
  firstStart: DOMPoint,
  firstEnd: DOMPoint,
  secondStart: DOMPoint,
  secondEnd: DOMPoint,
) {
  const rangesOverlap = (firstA: number, firstB: number, secondA: number, secondB: number) =>
    Math.max(Math.min(firstA, firstB), Math.min(secondA, secondB)) <=
    Math.min(Math.max(firstA, firstB), Math.max(secondA, secondB));
  if (
    !rangesOverlap(firstStart.x, firstEnd.x, secondStart.x, secondEnd.x) ||
    !rangesOverlap(firstStart.y, firstEnd.y, secondStart.y, secondEnd.y)
  ) {
    return false;
  }
  const cross = (start: DOMPoint, end: DOMPoint, point: DOMPoint) =>
    (end.x - start.x) * (point.y - start.y) - (end.y - start.y) * (point.x - start.x);
  const firstSideA = cross(firstStart, firstEnd, secondStart);
  const firstSideB = cross(firstStart, firstEnd, secondEnd);
  const secondSideA = cross(secondStart, secondEnd, firstStart);
  const secondSideB = cross(secondStart, secondEnd, firstEnd);
  return firstSideA * firstSideB <= 0 && secondSideA * secondSideB <= 0;
}

function segmentIntersectsRectangle(start: DOMPoint, end: DOMPoint, rectangle: SelectionRectangle) {
  if (pointInsideRectangle(start, rectangle) || pointInsideRectangle(end, rectangle)) return true;
  const topLeft = new DOMPoint(rectangle.left, rectangle.top);
  const topRight = new DOMPoint(rectangle.right, rectangle.top);
  const bottomRight = new DOMPoint(rectangle.right, rectangle.bottom);
  const bottomLeft = new DOMPoint(rectangle.left, rectangle.bottom);
  return (
    segmentsIntersect(start, end, topLeft, topRight) ||
    segmentsIntersect(start, end, topRight, bottomRight) ||
    segmentsIntersect(start, end, bottomRight, bottomLeft) ||
    segmentsIntersect(start, end, bottomLeft, topLeft)
  );
}

function edgeIntersectsRectangle(path: SVGPathElement, rectangle: SelectionRectangle) {
  const bounds = path.getBoundingClientRect();
  if (
    bounds.right < rectangle.left ||
    bounds.left > rectangle.right ||
    bounds.bottom < rectangle.top ||
    bounds.top > rectangle.bottom
  ) {
    return false;
  }
  const transform = path.getScreenCTM();
  if (!transform) return false;
  const length = path.getTotalLength();
  const sampleLength = 10;
  const initialPoint = path.getPointAtLength(0);
  let previous = new DOMPoint(initialPoint.x, initialPoint.y).matrixTransform(transform);
  for (let distance = sampleLength; distance < length + sampleLength; distance += sampleLength) {
    const point = path.getPointAtLength(Math.min(distance, length));
    const current = new DOMPoint(point.x, point.y).matrixTransform(transform);
    if (segmentIntersectsRectangle(previous, current, rectangle)) return true;
    previous = current;
  }
  return false;
}

function selectedAudioEdgeIds(root: HTMLElement | null, rectangle: SelectionRectangle) {
  const ids: string[] = [];
  const paths = root?.querySelectorAll<SVGPathElement>('.react-flow__edge .react-flow__edge-path');
  for (const path of paths ?? []) {
    if (!edgeIntersectsRectangle(path, rectangle)) continue;
    const id = path.closest<SVGGElement>('.react-flow__edge')?.dataset.id;
    if (id) ids.push(id);
  }
  return ids;
}

function selectedAudioPorts(
  root: HTMLElement | null,
  start: PortSelectionStart,
  end: { x: number; y: number },
) {
  const selectionRect = selectionRectangle(start, end);
  const handles = root?.querySelectorAll<HTMLElement>('[data-audio-port-side]');
  const hits: SelectedAudioPort[] = [];
  for (const handle of handles ?? []) {
    const bounds = handle.getBoundingClientRect();
    const misses =
      bounds.right < selectionRect.left ||
      bounds.left > selectionRect.right ||
      bounds.bottom < selectionRect.top ||
      bounds.top > selectionRect.bottom;
    if (misses) continue;
    const nodeId = handle.dataset.audioNodeId;
    const portId = handle.dataset.audioPortId;
    const side = handle.dataset.audioPortSide;
    if (nodeId && portId && (side === 'input' || side === 'output')) {
      hits.push({ nodeId, portId, side });
    }
  }
  return hits;
}

function mergeAudioPorts(
  current: readonly SelectedAudioPort[],
  added: readonly SelectedAudioPort[],
) {
  const combined = new Map(current.map((port) => [audioPortKey(port), port]));
  for (const port of added) combined.set(audioPortKey(port), port);
  return [...combined.values()];
}

function useAudioPortSelection(
  rootRef: { current: HTMLElement | null },
  onSelectEdges: (edgeIds: readonly string[]) => void,
) {
  const selectedRef = useRef<SelectedAudioPort[]>([]);
  const dragStartRef = useRef<PortSelectionStart | null>(null);
  const syncSelection = useCallback(
    (selected: readonly SelectedAudioPort[]) => {
      selectedRef.current = [...selected];
      const selectedKeys = new Set(selected.map(audioPortKey));
      const handles = rootRef.current?.querySelectorAll<HTMLElement>('[data-audio-port-side]');
      for (const handle of handles ?? []) {
        const side = handle.dataset.audioPortSide;
        if (side !== 'input' && side !== 'output') continue;
        const key = audioPortKey({
          nodeId: handle.dataset.audioNodeId ?? '',
          portId: handle.dataset.audioPortId ?? '',
          side,
        });
        handle.toggleAttribute('data-port-selected', selectedKeys.has(key));
      }
    },
    [rootRef],
  );
  const clear = useCallback(() => syncSelection([]), [syncSelection]);
  const onPointerDownCapture = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    dragStartRef.current = portSelectionStart(event);
  }, []);
  const onPointerUpCapture = useCallback(
    (event: ReactPointerEvent<HTMLElement>) => {
      const start = dragStartRef.current;
      dragStartRef.current = null;
      if (!start || start.pointerId !== event.pointerId) return;
      if (Math.hypot(event.clientX - start.x, event.clientY - start.y) <= portSelectionDistance) {
        return;
      }
      const end = {
        x: event.clientX,
        y: event.clientY,
      };
      const hits = selectedAudioPorts(rootRef.current, start, end);
      syncSelection(start.additive ? mergeAudioPorts(selectedRef.current, hits) : hits);
      if (start.additive) {
        onSelectEdges(selectedAudioEdgeIds(rootRef.current, selectionRectangle(start, end)));
      }
    },
    [onSelectEdges, rootRef, syncSelection],
  );
  const onPointerCancelCapture = useCallback(() => {
    dragStartRef.current = null;
  }, []);
  return {
    captureProps: { onPointerCancelCapture, onPointerDownCapture, onPointerUpCapture },
    clear,
    selectedRef,
    select: syncSelection,
  };
}

function useAudioFlowCollections(model: AudioGraphModel) {
  const initialNodes = useRef<AudioFlowNode[] | null>(null);
  const initialEdges = useRef<AudioFlowEdge[] | null>(null);
  initialNodes.current ??= reconcileAudioFlowNodes([], model);
  initialEdges.current ??= reconcileAudioFlowEdges([], model);
  const [nodes, setNodes, applyNodeChanges] = useNodesState<AudioFlowNode>(initialNodes.current);
  const [edges, setEdges, applyEdgeChanges] = useEdgesState<AudioFlowEdge>(initialEdges.current);

  useEffect(
    () => setNodes((current) => reconcileAudioFlowNodes(current, model)),
    [model, setNodes],
  );
  useEffect(
    () => setEdges((current) => reconcileAudioFlowEdges(current, model)),
    [model, setEdges],
  );
  return { applyEdgeChanges, applyNodeChanges, edges, nodes, setEdges, setNodes };
}

function toGraphConnection(connection: Connection | Edge): AudioGraphConnection {
  return {
    sourceNodeId: connection.source,
    sourcePortId: connection.sourceHandle ?? '',
    targetNodeId: connection.target,
    targetPortId: connection.targetHandle ?? '',
  };
}

function selectedNodePorts(
  model: AudioGraphModel,
  selectedPorts: readonly SelectedAudioPort[],
  nodeId: string,
  side: AudioPortSide,
) {
  const node = model.nodes.find((candidate) => candidate.id === nodeId);
  if (!node) return [];
  const selectedIds = new Set(
    selectedPorts
      .filter((port) => port.nodeId === nodeId && port.side === side)
      .map((port) => port.portId),
  );
  const ports = side === 'input' ? node.inputPorts : node.outputPorts;
  return ports.filter((port) => selectedIds.has(port.id));
}

function portVerticalOffset(
  model: AudioGraphModel,
  nodeId: string,
  side: AudioPortSide,
  portId: string,
  anchorPortId: string,
) {
  const node = model.nodes.find((candidate) => candidate.id === nodeId);
  if (!node) return 0;
  const ports = side === 'input' ? node.inputPorts : node.outputPorts;
  const portIndex = ports.findIndex((port) => port.id === portId);
  const anchorIndex = ports.findIndex((port) => port.id === anchorPortId);
  if (portIndex < 0 || anchorIndex < 0) return 0;
  const geometry = graphNodeFaceGeometry(node);
  return (
    audioNodePortTop(geometry, portIndex, ports.length) -
    audioNodePortTop(geometry, anchorIndex, ports.length)
  );
}

function relativeTargetPort(
  model: AudioGraphModel,
  sourceNodeId: string,
  sourcePortId: string,
  sourceAnchorId: string,
  targetNodeId: string,
  targetAnchorId: string,
) {
  const sourcePorts = model.nodes.find((node) => node.id === sourceNodeId)?.outputPorts;
  const targetPorts = model.nodes.find((node) => node.id === targetNodeId)?.inputPorts;
  const sourceIndex = sourcePorts?.findIndex((port) => port.id === sourcePortId) ?? -1;
  const sourceAnchorIndex = sourcePorts?.findIndex((port) => port.id === sourceAnchorId) ?? -1;
  const targetAnchorIndex = targetPorts?.findIndex((port) => port.id === targetAnchorId) ?? -1;
  if (!targetPorts || sourceIndex < 0 || sourceAnchorIndex < 0 || targetAnchorIndex < 0) {
    return undefined;
  }
  return targetPorts[targetAnchorIndex + sourceIndex - sourceAnchorIndex];
}

interface ConnectionPreviewPair {
  sourcePort: Pick<AudioGraphPort, 'id'>;
  targetPort?: AudioGraphPort;
}

function connectionPreviewPairs(
  model: AudioGraphModel,
  selectedPorts: readonly SelectedAudioPort[],
  sourceNodeId: string,
  sourceAnchorId: string,
  targetNodeId?: string,
  targetAnchorId?: string,
): ConnectionPreviewPair[] {
  const selectedSources = selectedNodePorts(model, selectedPorts, sourceNodeId, 'output');
  const activeSources =
    selectedSources.length > 1 && selectedSources.some((port) => port.id === sourceAnchorId)
      ? selectedSources
      : [{ id: sourceAnchorId }];
  if (!targetNodeId || !targetAnchorId) {
    return activeSources.map((sourcePort) => ({ sourcePort }));
  }
  const selectedTargets = selectedNodePorts(model, selectedPorts, targetNodeId, 'input');
  if (selectedTargets.some((port) => port.id === targetAnchorId)) {
    return activeSources.slice(0, selectedTargets.length).map((sourcePort, index) => ({
      sourcePort,
      targetPort: selectedTargets[index],
    }));
  }
  return activeSources.flatMap((sourcePort) => {
    const targetPort = relativeTargetPort(
      model,
      sourceNodeId,
      sourcePort.id,
      sourceAnchorId,
      targetNodeId,
      targetAnchorId,
    );
    return targetPort ? [{ sourcePort, targetPort }] : [];
  });
}

function selectedConnectionBatch(
  model: AudioGraphModel,
  connection: AudioGraphConnection,
  selectedPorts: readonly SelectedAudioPort[],
): AudioGraphConnection[] {
  const selectedOutputs = selectedNodePorts(
    model,
    selectedPorts,
    connection.sourceNodeId,
    'output',
  );
  if (
    selectedOutputs.length < 2 ||
    !selectedOutputs.some((port) => port.id === connection.sourcePortId)
  ) {
    return [connection];
  }
  const sourceNode = model.nodes.find((node) => node.id === connection.sourceNodeId);
  const targetNode = model.nodes.find((node) => node.id === connection.targetNodeId);
  const sourceAnchorIndex = sourceNode?.outputPorts.findIndex(
    (port) => port.id === connection.sourcePortId,
  );
  const targetAnchorIndex = targetNode?.inputPorts.findIndex(
    (port) => port.id === connection.targetPortId,
  );
  if (
    !sourceNode ||
    !targetNode ||
    sourceAnchorIndex === undefined ||
    sourceAnchorIndex < 0 ||
    targetAnchorIndex === undefined ||
    targetAnchorIndex < 0
  ) {
    return [connection];
  }

  const selectedInputs = selectedNodePorts(model, selectedPorts, connection.targetNodeId, 'input');
  if (
    selectedInputs.length > 0 &&
    selectedInputs.some((port) => port.id === connection.targetPortId)
  ) {
    return selectedOutputs.slice(0, selectedInputs.length).flatMap((sourcePort, index) => {
      const targetPort = selectedInputs[index];
      return targetPort
        ? [
            {
              sourceNodeId: sourceNode.id,
              sourcePortId: sourcePort.id,
              targetNodeId: targetNode.id,
              targetPortId: targetPort.id,
            },
          ]
        : [];
    });
  }

  return selectedOutputs.flatMap((sourcePort) => {
    const targetPort = relativeTargetPort(
      model,
      sourceNode.id,
      sourcePort.id,
      connection.sourcePortId,
      targetNode.id,
      connection.targetPortId,
    );
    return targetPort
      ? [
          {
            sourceNodeId: sourceNode.id,
            sourcePortId: sourcePort.id,
            targetNodeId: targetNode.id,
            targetPortId: targetPort.id,
          },
        ]
      : [];
  });
}

function BatchConnectionLine({
  connectionLineStyle,
  connectionStatus,
  fromHandle,
  fromNode,
  fromPosition,
  fromX,
  fromY,
  toHandle,
  toNode,
  toPosition,
  toX,
  toY,
}: ConnectionLineComponentProps<AudioFlowNode>) {
  const selection = useContext(AudioPortSelectionContext);
  const { zoom } = useViewport();
  const sourceAnchorId = fromHandle.id ?? '';
  const targetAnchorId = toHandle?.id ?? '';
  const previews = selection
    ? connectionPreviewPairs(
        selection.model,
        selection.selectedRef.current,
        fromNode.id,
        sourceAnchorId,
        toNode?.id,
        targetAnchorId,
      )
    : [{ sourcePort: { id: sourceAnchorId } }];

  return (
    <>
      {previews.map(({ sourcePort, targetPort }, index) => {
        const sourceOffset = selection
          ? portVerticalOffset(
              selection.model,
              fromNode.id,
              'output',
              sourcePort.id,
              sourceAnchorId,
            ) * zoom
          : 0;
        const targetOffset =
          selection && toNode && targetPort
            ? portVerticalOffset(
                selection.model,
                toNode.id,
                'input',
                targetPort.id,
                targetAnchorId,
              ) * zoom
            : 0;
        const [path] = getBezierPath({
          sourcePosition: fromPosition,
          sourceX: fromX,
          sourceY: fromY + sourceOffset,
          targetPosition: toPosition,
          targetX: toX,
          targetY: toY + targetOffset,
        });
        return (
          <path
            className="react-flow__connection-path"
            data-connection-status={connectionStatus ?? undefined}
            d={path}
            fill="none"
            key={`${sourcePort.id}:${targetPort?.id ?? index}`}
            style={connectionLineStyle}
          />
        );
      })}
    </>
  );
}

function useConnectionHandlers(
  model: AudioGraphModel,
  commit: CommitGraph,
  selectedPortsRef: { current: SelectedAudioPort[] },
  clearSelectedPorts: () => void,
) {
  const isValidConnection = useCallback(
    (connection: Connection | Edge) =>
      canConnectAudioGraph(
        model,
        toGraphConnection(connection),
        'id' in connection ? connection.id : undefined,
      ),
    [model],
  );
  const onConnect = useCallback(
    (connection: Connection) => {
      const connections = selectedConnectionBatch(
        model,
        toGraphConnection(connection),
        selectedPortsRef.current,
      );
      const nextModel = connectAudioGraphBatch(model, connections);
      commit(nextModel, 'runtime');
      if (nextModel !== model && connections.length > 1) clearSelectedPorts();
    },
    [clearSelectedPorts, commit, model, selectedPortsRef],
  );
  const onReconnect = useCallback(
    (edge: AudioFlowEdge, connection: Connection) =>
      commit(reconnectAudioGraph(model, edge.id, toGraphConnection(connection)), 'runtime'),
    [commit, model],
  );
  const onReconnectEnd = useCallback<ReconnectEnd>(
    (_, edge, _handleType, connectionState) => {
      if (!connectionState.isValid) commit(disconnectAudioGraph(model, [edge.id]), 'runtime');
    },
    [commit, model],
  );
  return { isValidConnection, onConnect, onReconnect, onReconnectEnd };
}

function useDeletionHandlers(model: AudioGraphModel, commit: CommitGraph) {
  const removeEdges = useCallback(
    (removedEdges: AudioFlowEdge[]) =>
      commit(
        disconnectAudioGraph(
          model,
          removedEdges.map((edge) => edge.id),
        ),
        'runtime',
      ),
    [commit, model],
  );
  const onDelete = useCallback(
    ({ nodes, edges }: { nodes: AudioFlowNode[]; edges: AudioFlowEdge[] }) =>
      commit(
        removeAudioGraphItems(
          model,
          nodes.map((node) => node.id),
          edges.map((edge) => edge.id),
        ),
        'runtime',
      ),
    [commit, model],
  );
  const onEdgeDoubleClick = useCallback(
    (_: ReactMouseEvent, edge: AudioFlowEdge) => removeEdges([edge]),
    [removeEdges],
  );
  return { onDelete, onEdgeDoubleClick };
}

function useNodeChangeHandler(
  model: AudioGraphModel,
  commit: CommitGraph,
  applyNodeChanges: (changes: NodeChange<AudioFlowNode>[]) => void,
) {
  return useCallback(
    (changes: NodeChange<AudioFlowNode>[]) => {
      applyNodeChanges(changes);
      let nextModel = model;
      for (const change of changes) {
        if (change.type === 'position' && change.position) {
          nextModel = moveAudioGraphNode(nextModel, change.id, change.position);
        }
        if (change.type === 'dimensions' && change.dimensions && change.setAttributes) {
          nextModel = resizeAudioGraphNode(nextModel, change.id, change.dimensions);
        }
      }
      commit(nextModel, 'layout');
    },
    [applyNodeChanges, commit, model],
  );
}

function AddNodePanel({
  addNode,
  label,
  nodeMenu,
  nodeMenuItems,
}: {
  addNode: (itemId: string) => void;
  label?: string;
  nodeMenu: ReturnType<typeof useNodeMenu>;
  nodeMenuItems: readonly AudioGraphNodeMenuItem[];
}) {
  if (nodeMenuItems.length === 0) return null;
  return (
    <ReactFlowPanel className={`${styles.nodeToolbar} nodrag nopan nowheel`} position="top-left">
      <HierarchicalMenuButton
        buttonClassName={`${styles.addNode} nodrag nopan nowheel`}
        buttonContent={<span aria-hidden="true">+</span>}
        items={nodeMenuItems}
        label={label ?? ''}
        onClick={nodeMenu.prepareButtonPosition}
        onOpenedChange={nodeMenu.setButtonOpened}
        onSelect={addNode}
        opened={nodeMenu.buttonOpened}
        tooltip={label}
      />
    </ReactFlowPanel>
  );
}

function GraphCanvasBackground({
  addNode,
  label,
  nodeMenu,
  nodeMenuItems,
}: Parameters<typeof AddNodePanel>[0]) {
  return (
    <>
      <Background color="#24282e" gap={24} size={1} variant={BackgroundVariant.Lines} />
      <AddNodePanel
        addNode={addNode}
        label={label}
        nodeMenu={nodeMenu}
        nodeMenuItems={nodeMenuItems}
      />
    </>
  );
}

function useEdgeBoxSelection(setEdges: Dispatch<SetStateAction<AudioFlowEdge[]>>) {
  return useCallback(
    (edgeIds: readonly string[]) => {
      if (edgeIds.length === 0) return;
      const selectedIds = new Set(edgeIds);
      requestAnimationFrame(() => {
        setEdges((current) =>
          current.map((edge) =>
            selectedIds.has(edge.id) && !edge.selected ? { ...edge, selected: true } : edge,
          ),
        );
      });
    },
    [setEdges],
  );
}

function GraphContextSurface({
  children,
  contextMenu,
  nodeMenu,
  addNode,
  ariaLabel,
  captureProps,
}: {
  children: ReactNode;
  contextMenu: ReturnType<typeof useGraphContextMenu>;
  nodeMenu: ReturnType<typeof useNodeMenu>;
  addNode: (id: string) => void;
  ariaLabel: string;
  captureProps: ReturnType<typeof useAudioPortSelection>['captureProps'];
}) {
  return (
    <HierarchicalContextMenu
      items={contextMenu.items}
      onOpenedChange={nodeMenu.setContextOpened}
      onSelect={(id) => {
        nodeMenu.close();
        if (!contextMenu.execute(id)) addNode(id);
      }}
      opened={nodeMenu.contextOpened}
    >
      <section
        {...captureProps}
        onPointerDownCapture={(event) => {
          // The whole canvas is the context trigger, so the menu's outside-click
          // handler cannot dismiss clicks on other nodes or ports inside it.
          if (event.button !== 2) nodeMenu.setContextOpened(false);
          captureProps.onPointerDownCapture(event);
        }}
        aria-label={ariaLabel}
        className={styles.root}
        ref={nodeMenu.rootRef}
        onContextMenu={(event) => {
          contextMenu.prepare(event);
          nodeMenu.prepareContextPosition(event);
        }}
        onTouchStart={(event) => {
          // Mantine's long-press opens without dispatching a contextmenu event.
          // Capture the target and position for that path as well.
          const touch = event.touches[0];
          if (!touch) return;
          contextMenu.prepare(event);
          nodeMenu.prepareContextPosition(touch);
        }}
        onContextMenuCapture={(event) => {
          // Native text editing menus must not act on the enclosing node.
          if (
            event.target instanceof Element &&
            event.target.closest('input, textarea, [contenteditable="true"]')
          )
            event.stopPropagation();
        }}
      >
        {children}
      </section>
    </HierarchicalContextMenu>
  );
}

function restoreCanvasViewport(
  instance: ReactFlowInstance<AudioFlowNode, AudioFlowEdge> | null,
  viewport: AudioGraphViewport | undefined,
) {
  if (!instance || !viewport) return;
  const current = instance.getViewport();
  if (current.x === viewport.x && current.y === viewport.y && current.zoom === viewport.zoom)
    return;
  // Acknowledging a saved gesture must not animate or reposition an already matching view.
  void instance.setViewport({ ...viewport }, { duration: 0 });
}

function useCanvasViewport(
  viewport: AudioGraphViewport | undefined,
  onViewportChange: AudioGraphCanvasProps['onViewportChange'],
  nodeMenu: Pick<ReturnType<typeof useNodeMenu>, 'flowRef' | 'setFlowInstance'>,
) {
  const { flowRef, setFlowInstance } = nodeMenu;
  const mounted = useRef(true);
  const requestedViewport = useRef(viewport);
  requestedViewport.current = viewport;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const onInit = useCallback(
    (instance: ReactFlowInstance<AudioFlowNode, AudioFlowEdge>) => {
      if (!mounted.current) return;
      setFlowInstance(instance);
      // React Flow can finish initializing after the external document has changed.
      restoreCanvasViewport(instance, requestedViewport.current);
    },
    [setFlowInstance],
  );
  const x = viewport?.x;
  const y = viewport?.y;
  const zoom = viewport?.zoom;
  useEffect(() => {
    if (x !== undefined && y !== undefined && zoom !== undefined)
      restoreCanvasViewport(flowRef.current, { x, y, zoom });
  }, [flowRef, x, y, zoom]);
  const onMoveEnd = useCallback<
    NonNullable<ReactFlowProps<AudioFlowNode, AudioFlowEdge>['onMoveEnd']>
  >(
    (event, nextViewport) => {
      // XYFlow's programmatic setViewport/fitView events have no source event.
      // Using onMoveEnd rather than onMove also avoids an autosave for every frame.
      if (event && mounted.current) onViewportChange?.({ ...nextViewport });
    },
    [onViewportChange],
  );
  return { onInit, onMoveEnd };
}

function GraphFlowSurface(props: ReactFlowProps<AudioFlowNode, AudioFlowEdge>) {
  return (
    <ReactFlow<AudioFlowNode, AudioFlowEdge>
      {...props}
      colorMode="dark"
      proOptions={{ hideAttribution: true }}
      connectionLineComponent={BatchConnectionLine}
      connectionMode={ConnectionMode.Strict}
      deleteKeyCode={deleteKeyCode}
      edgesReconnectable
      fitView={props.fitView ?? true}
      // Match the persisted layout contract; React Flow's defaults would clamp restored views.
      minZoom={0.05}
      maxZoom={8}
      multiSelectionKeyCode="Shift"
      nodeTypes={nodeTypes}
      panActivationKeyCode={panActivationKeyCode}
      panOnDrag={panMouseButtons}
      reconnectRadius={18}
      selectionOnDrag
      snapGrid={snapGrid}
      snapToGrid
    />
  );
}

export function AudioGraphCanvas({
  addNodeLabel,
  contextMenuLabels,
  ariaLabel,
  meterStore,
  model,
  nodeMenuItems = [],
  onAddNode,
  onModelChange,
  onNodeMenuOpenedChange,
  onNodeEndpointChange,
  onNodeGainChange,
  onNodeParameterChange,
  onProcessorEditorRetry,
  processorEditorLabels,
  processorEditorStatuses = emptyProcessorEditorStatuses,
  renderNodeContent,
  viewport,
  onViewportChange,
}: AudioGraphCanvasProps) {
  const nodeMenu = useNodeMenu();
  const viewportEvents = useCanvasViewport(viewport, onViewportChange, nodeMenu);
  const menuOpened = nodeMenu.buttonOpened || nodeMenu.contextOpened;
  useEffect(() => onNodeMenuOpenedChange?.(menuOpened), [menuOpened, onNodeMenuOpenedChange]);
  const { applyEdgeChanges, applyNodeChanges, edges, nodes, setEdges, setNodes } =
    useAudioFlowCollections(model);
  const selectEdges = useEdgeBoxSelection(setEdges);
  const portSelection = useAudioPortSelection(nodeMenu.rootRef, selectEdges);
  const commit = useCallback(
    (nextModel: AudioGraphModel, kind: AudioGraphChangeKind) => {
      if (nextModel !== model) onModelChange(nextModel, kind);
    },
    [model, onModelChange],
  );
  const { isValidConnection, onConnect, onReconnect, onReconnectEnd } = useConnectionHandlers(
    model,
    commit,
    portSelection.selectedRef,
    portSelection.clear,
  );
  const { onDelete, onEdgeDoubleClick } = useDeletionHandlers(model, commit);
  const onNodesChange = useNodeChangeHandler(model, commit, applyNodeChanges);
  const { addNode, editorContext, onPaneClick } = useCanvasViewActions(
    onAddNode,
    onProcessorEditorRetry,
    processorEditorLabels,
    processorEditorStatuses,
    nodeMenu,
    portSelection.clear,
  );
  const contextMenu = useGraphContextMenu({
    model,
    nodes,
    edges,
    setNodes,
    setEdges,
    addItems: nodeMenuItems,
    labels: contextMenuLabels,
    commit,
    clearPorts: portSelection.clear,
    selectPorts: portSelection.select,
    selectedPorts: portSelection.selectedRef,
    fitView: () => {
      void nodeMenu.flowRef.current?.fitView({ padding: 0.15 });
    },
  });

  return (
    <GraphContextSurface
      contextMenu={contextMenu}
      nodeMenu={nodeMenu}
      addNode={addNode}
      ariaLabel={ariaLabel}
      captureProps={portSelection.captureProps}
    >
      <AudioGraphProviders
        editor={editorContext}
        meterStore={meterStore ?? null}
        model={model}
        onEndpointChange={onNodeEndpointChange}
        onGainChange={onNodeGainChange}
        onParameterChange={onNodeParameterChange}
        selectedRef={portSelection.selectedRef}
        renderNodeContent={renderNodeContent}
      >
        <GraphFlowSurface
          {...viewportEvents}
          defaultViewport={viewport}
          edges={edges}
          fitView={viewport === undefined}
          isValidConnection={isValidConnection}
          nodes={nodes}
          onConnect={onConnect}
          onDelete={onDelete}
          onEdgeDoubleClick={onEdgeDoubleClick}
          onEdgesChange={applyEdgeChanges}
          onNodesChange={onNodesChange}
          onPaneClick={onPaneClick}
          onReconnect={onReconnect}
          onReconnectEnd={onReconnectEnd}
        >
          <GraphCanvasBackground
            addNode={addNode}
            label={addNodeLabel}
            nodeMenu={nodeMenu}
            nodeMenuItems={nodeMenuItems}
          />
        </GraphFlowSurface>
      </AudioGraphProviders>
    </GraphContextSurface>
  );
}

function AudioFlowNodeView({ data, selected }: NodeProps<AudioFlowNode>) {
  const { graphNode } = data;
  const onNodeGainChange = useContext(NodeGainContext);
  const onNodeEndpointChange = useContext(NodeEndpointContext);
  const onNodeParameterChange = useContext(NodeParameterContext);
  const processorEditors = useContext(NodeProcessorEditorContext);
  const renderNodeContent = useContext(NodeContentContext);
  const updateNodeInternals = useUpdateNodeInternals();
  const editorStatus = processorEditors?.statuses.get(graphNode.id);
  const editorWidth = editorStatus?.state === 'open' ? editorStatus.width : null;
  const layout = useAudioNodeLayout(graphNode, {
    width: editorWidth,
    height: editorStatus?.height ?? null,
  });
  const levels = useAudioMeterLevels(
    graphNode.appearance.meterSource?.nodeId,
    graphNode.appearance.meterSource?.portId,
  );
  return (
    <div
      ref={layout.ref}
      className={styles.node}
      style={{
        width:
          graphNode.size?.width ??
          (editorWidth ? Math.min(1600, Math.max(240, editorWidth)) : undefined),
        height: graphNode.size?.height,
        minHeight: layout.minHeight,
      }}
    >
      {graphNode.inputPorts.map((port, index) => (
        <AudioPortHandle
          top={audioNodePortTop(layout.ports, index, graphNode.inputPorts.length)}
          index={index}
          key={port.id}
          nodeId={graphNode.id}
          port={port}
          side="input"
        />
      ))}
      <AudioNodeCard
        {...graphNode.appearance}
        content={renderNodeContent?.(graphNode)}
        faceHeight={layout.face.height}
        faceMode={graphNodeFaceMode(graphNode)}
        onFaceHeightChange={layout.onFaceHeightChange}
        onContentHeightChange={layout.onContentHeightChange}
        inputCount={graphNode.inputPorts.length}
        meterLevels={levels}
        onGainChange={
          onNodeGainChange ? (gainDb) => onNodeGainChange(graphNode.id, gainDb) : undefined
        }
        onEndpointChange={
          onNodeEndpointChange ? (value) => onNodeEndpointChange(graphNode.id, value) : undefined
        }
        onLayoutChange={() => updateNodeInternals(graphNode.id)}
        onParameterChange={
          onNodeParameterChange
            ? (parameterId, value) => onNodeParameterChange(graphNode.id, parameterId, value)
            : undefined
        }
        onProcessorEditorRetry={
          graphNode.appearance.hasProcessorEditor && processorEditors
            ? () => processorEditors.onRetry(graphNode.id)
            : undefined
        }
        onSelect={() => undefined}
        outputCount={graphNode.outputPorts.length}
        processorEditor={processorEditorProps(
          graphNode.id,
          graphNode.appearance.hasProcessorEditor,
          processorEditors,
        )}
        selected={selected}
        showPorts={false}
      />
      {graphNode.outputPorts.map((port, index) => (
        <AudioPortHandle
          top={audioNodePortTop(layout.ports, index, graphNode.outputPorts.length)}
          index={index}
          key={port.id}
          nodeId={graphNode.id}
          port={port}
          side="output"
        />
      ))}
      <NodeResizeGrip minHeight={layout.minHeight} />
    </div>
  );
}

function NodeResizeGrip({ minHeight }: { minHeight: number }) {
  return (
    <NodeResizeControl
      className={`${styles.resizeHandle} nodrag nopan`}
      position="bottom-right"
      minWidth={180}
      minHeight={minHeight}
      maxWidth={4096}
      maxHeight={4096}
      autoScale={false}
    >
      <svg aria-hidden="true" width="12" height="12" viewBox="0 0 12 12">
        <path d="m3 10 7-7M7 10l3-3" fill="none" stroke="currentColor" />
      </svg>
    </NodeResizeControl>
  );
}

function processorEditorProps(
  nodeId: string,
  available: boolean | undefined,
  context: NodeProcessorEditorContextValue | null,
) {
  if (!available || !context) return undefined;
  const status = context.statuses.get(nodeId) ?? {
    failure: null,
    height: null,
    nodeId,
    protocolVersion: 1 as const,
    resizable: false,
    state: 'closed' as const,
    width: null,
  };
  return {
    actionLabel: context.labels.retry,
    failureLabel: status.failure ? context.labels.failure[status.failure] : undefined,
    state: status.state,
  };
}

function AudioPortHandle({
  top,
  index,
  nodeId,
  port,
  side,
}: {
  top: number;
  index: number;
  nodeId: string;
  port: AudioGraphPort;
  side: 'input' | 'output';
}) {
  const channelNumber = (port.channelIndex ?? index) + 1;
  const isConnectionTarget = useConnection(
    (connection) =>
      connection.inProgress &&
      connection.toHandle?.nodeId === nodeId &&
      connection.toHandle.id === port.id,
  );

  return (
    <Tooltip
      classNames={portTooltipClassNames}
      label={`CH ${channelNumber}`}
      offset={7}
      opened={isConnectionTarget ? true : undefined}
      openDelay={isConnectionTarget ? 0 : 160}
      position={side === 'input' ? 'left' : 'right'}
      transitionProps={portTooltipTransition}
    >
      <Handle
        aria-label={`${port.label}, CH ${channelNumber}`}
        className={`${styles.handle} ${side === 'input' ? styles.inputHandle : ''}`}
        data-audio-node-id={nodeId}
        data-audio-port-id={port.id}
        data-audio-port-side={side}
        data-connection-target={isConnectionTarget || undefined}
        id={port.id}
        position={side === 'input' ? Position.Left : Position.Right}
        style={{
          top: `${top}px`,
        }}
        type={side === 'input' ? 'target' : 'source'}
      />
    </Tooltip>
  );
}
