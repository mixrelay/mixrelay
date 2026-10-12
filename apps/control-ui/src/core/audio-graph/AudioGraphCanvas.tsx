import '@xyflow/react/dist/base.css';

import {
  type AriaLabelConfig,
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
  type XYPosition,
} from '@xyflow/react';
import {
  createContext,
  type Dispatch,
  memo,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  type RefObject,
  type SetStateAction,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import { AudioMeterProvider } from '../audio-meter/AudioMeterProvider';
import type { AudioMeterStore } from '../audio-meter/AudioMeterStore';
import type { ProcessorEditorFailure, ProcessorEditorStatus } from '../control-port/types';
import { AudioNodeCard } from '../ui/AudioNodeCard';
import type { EndpointUpdate } from '../ui/AudioNodeEndpointOptions';
import { audioNodePortTop } from '../ui/audioNodeLayout';
import {
  HierarchicalContextMenu,
  HierarchicalMenuButton,
  type HierarchicalMenuItem,
} from '../ui/HierarchicalMenu';
import {
  NodeOptionsContext,
  NodeOptionsIdContext,
  type NodeOptionsState,
} from '../ui/nodeOptionsState';
import { platformModifier } from '../ui/platformShortcut';
import { Tooltip } from '../ui/Tooltip';
import styles from './AudioGraphCanvas.module.css';
import { ChannelConnectionDialog } from './ChannelConnectionDialog';
import {
  type ChannelConnectionRequest,
  mapChannelConnections,
  planChannelConnections,
} from './channelConnections';
import {
  focusGraphNode,
  openContextMenuFromKeyboard,
  useChannelConnectionRequests,
} from './graphKeyboard';
import {
  type AudioGraphConnection,
  type AudioGraphModel,
  type AudioGraphNode,
  type AudioGraphPort,
  audioGraphNodeTitle,
  canConnectAudioGraph,
  connectAudioGraphBatch,
  disconnectAudioGraph,
  graphNodeMinimumWidth,
  moveAudioGraphNode,
  normalizeAudioGraphNodeSize,
  reconnectAudioGraph,
  removeAudioGraphItems,
  resizeAudioGraphNode,
} from './model';
import { PortSelectionFeedback } from './PortSelectionFeedback';
import {
  type AudioFlowEdge,
  type AudioFlowEdgeLabel,
  type AudioFlowNode,
  type AudioGraphEdgeEndpoints,
  applyAudioNodeLayers,
  namedAudioPorts,
  reconcileAudioFlowEdges,
  reconcileAudioFlowNodes,
} from './reactFlowAdapter';
import { graphNodeFaceMode, useAudioNodeLayout } from './useAudioNodeLayout';
import { type SelectedAudioPort, useAudioPortSelection } from './useAudioPortSelection';
import { type GraphContextMenuLabels, useGraphContextMenu } from './useGraphContextMenu';

const nodeTypes = { audio: memo(AudioFlowNodeView) };
const deleteKeyCode = ['Backspace', 'Delete'];
const fitViewOptions = { padding: 0.15, maxZoom: 1 };
const panMouseButtons = [1];
const snapGrid: [number, number] = [12, 12];
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
const NodeParameterContext = createContext<{
  single?: (nodeId: string, parameterId: string, value: number) => void;
  batch?: (nodeId: string, updates: readonly { id: string; value: number }[]) => void;
}>({});
export interface ProcessorEditorLabels {
  open: string;
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
export interface AudioNodeControlSlots {
  power?: ReactNode;
  mute?: ReactNode;
}
const NodeControlsContext = createContext<
  ((node: AudioGraphNode) => AudioNodeControlSlots | null) | undefined
>(undefined);
export type AudioGraphChangeKind =
  | 'layout'
  | 'level'
  | 'processor_parameter'
  | 'parameter_group'
  | 'runtime';
export interface AudioGraphViewport {
  x: number;
  y: number;
  zoom: number;
}
type CommitGraph = (nextModel: AudioGraphModel, kind: AudioGraphChangeKind) => void;
type ReconnectEnd = NonNullable<ReactFlowProps<AudioFlowNode, AudioFlowEdge>['onReconnectEnd']>;

type AudioPortSide = 'input' | 'output';

interface AudioPortSelectionContextValue {
  model: AudioGraphModel;
  selectedRef: { current: SelectedAudioPort[] };
}

const AudioPortSelectionContext = createContext<AudioPortSelectionContextValue | null>(null);
const NodeDisclosureHeightsContext = createContext<Map<string, number> | null>(null);

export interface AudioGraphCanvasProps {
  renderNodeControls?: (node: AudioGraphNode) => AudioNodeControlSlots | null;
  renderNodeContent?: (node: AudioGraphNode) => ReactNode;
  contextMenuLabels?: GraphContextMenuLabels;
  addNodeLabel?: string;
  ariaLabel: string;
  meterStore?: AudioMeterStore;
  model: AudioGraphModel;
  nodeMenuItems?: readonly AudioGraphNodeMenuItem[];
  onAddNode?: (itemId: string, position: XYPosition) => void;
  onNodeMenuOpenedChange?: (opened: boolean) => void;
  onNodeRenameRequest?: (nodeId: string) => void;
  onModelChange: (model: AudioGraphModel, kind: AudioGraphChangeKind) => void;
  onNodeEndpointChange?: (nodeId: string, value: EndpointUpdate) => void;
  onNodeGainChange?: (nodeId: string, gainDb: number) => void;
  onNodeParameterChange?: (nodeId: string, parameterId: string, value: number) => void;
  onNodeParametersChange?: (
    nodeId: string,
    updates: readonly { id: string; value: number }[],
  ) => void;
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
  options,
  editor,
  meterStore,
  model,
  onEndpointChange,
  onGainChange,
  onParameterChange,
  onParametersChange,
  selectedRef,
  renderNodeContent,
  renderNodeControls,
  disclosureHeights,
}: {
  children: ReactNode;
  options: NodeOptionsState;
  editor: NodeProcessorEditorContextValue | null;
  meterStore: AudioMeterStore | null;
  model: AudioGraphModel;
  onEndpointChange: AudioGraphCanvasProps['onNodeEndpointChange'];
  onGainChange: AudioGraphCanvasProps['onNodeGainChange'];
  onParameterChange: AudioGraphCanvasProps['onNodeParameterChange'];
  onParametersChange: AudioGraphCanvasProps['onNodeParametersChange'];
  selectedRef: AudioPortSelectionContextValue['selectedRef'];
  renderNodeContent: AudioGraphCanvasProps['renderNodeContent'];
  renderNodeControls: AudioGraphCanvasProps['renderNodeControls'];
  disclosureHeights: Map<string, number>;
}) {
  return (
    <AudioMeterProvider store={meterStore}>
      <NodeProcessorEditorContext.Provider value={editor}>
        <NodeParameterContext.Provider
          value={{ single: onParameterChange, batch: onParametersChange }}
        >
          <NodeEndpointContext.Provider value={onEndpointChange}>
            <NodeGainContext.Provider value={onGainChange}>
              <AudioPortSelectionContext.Provider value={{ model, selectedRef }}>
                <NodeContentContext.Provider value={renderNodeContent}>
                  <NodeControlsContext.Provider value={renderNodeControls}>
                    <NodeOptionsContext.Provider value={options}>
                      <NodeDisclosureHeightsContext.Provider value={disclosureHeights}>
                        {children}
                      </NodeDisclosureHeightsContext.Provider>
                    </NodeOptionsContext.Provider>
                  </NodeControlsContext.Provider>
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

function useNodeMenu(onOpenedChange?: (opened: boolean) => void) {
  const rootRef = useRef<HTMLElement | null>(null);
  const flowRef = useRef<ReactFlowInstance<AudioFlowNode, AudioFlowEdge> | null>(null);
  const positionRef = useRef<XYPosition>({ x: 0, y: 0 });
  const [buttonOpened, setButtonOpened] = useState(false);
  const [contextOpened, setContextOpened] = useState(false);
  const opened = buttonOpened || contextOpened;
  useEffect(() => onOpenedChange?.(opened), [opened, onOpenedChange]);
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
    // These handlers run from the mounted canvas and its child trigger.
    const bounds = (rootRef.current as HTMLElement).getBoundingClientRect();
    const screenPosition = {
      x: event.clientX - bounds.left,
      y: event.clientY - bounds.top,
    };
    positionRef.current =
      flowRef.current?.screenToFlowPosition({ x: event.clientX, y: event.clientY }) ??
      screenPosition;
  }, []);
  const prepareButtonPosition = useCallback((event: ReactMouseEvent<Element>) => {
    event.stopPropagation();
    setContextOpened(false);
    // These handlers run from the mounted canvas and its child trigger.
    const bounds = (rootRef.current as HTMLElement).getBoundingClientRect();
    const screenPosition = { x: 132, y: 92 };
    positionRef.current =
      flowRef.current?.screenToFlowPosition({
        x: bounds.left + screenPosition.x,
        y: bounds.top + screenPosition.y,
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

function useEdgeLabel(): AudioFlowEdgeLabel {
  const { t } = useTranslation();
  return useCallback(
    ({ source, sourcePort, target, targetPort, ...channels }: AudioGraphEdgeEndpoints) =>
      t('routing.a11y.edgeLabel', {
        ...channels,
        // Bus names disambiguate repeated channel numbers, matching the port's own name.
        source: sourcePort ? `${source} · ${sourcePort}` : source,
        target: targetPort ? `${target} · ${targetPort}` : target,
      }),
    [t],
  );
}

/** React Flow's built-in keyboard and live-region text follows the app language. */
function useGraphAriaLabels(): Partial<AriaLabelConfig> {
  const { t } = useTranslation();
  return useMemo(
    () => ({
      'node.a11yDescription.default': t('routing.a11y.nodeDescriptionStatic'),
      'node.a11yDescription.keyboardDisabled': t('routing.a11y.nodeDescription'),
      'node.a11yDescription.ariaLiveMessage': ({ direction, x, y }) =>
        t('routing.a11y.nodeMoved', {
          direction: t(`routing.a11y.directions.${direction as ArrowDirection}`),
          x,
          y,
        }),
      'edge.a11yDescription.default': t('routing.a11y.edgeDescription'),
    }),
    [t],
  );
}

type ArrowDirection = 'up' | 'down' | 'left' | 'right';

function useAudioFlowCollections(
  model: AudioGraphModel,
  commit: CommitGraph,
  disclosureHeights: Map<string, number>,
  openedNodeId: string | null,
) {
  const edgeLabel = useEdgeLabel();
  const initialNodes = useRef<AudioFlowNode[] | null>(null);
  const initialEdges = useRef<AudioFlowEdge[] | null>(null);
  initialNodes.current ??= reconcileAudioFlowNodes([], model);
  initialEdges.current ??= reconcileAudioFlowEdges([], model, edgeLabel);
  const [nodes, setNodes, applyNodeChanges] = useNodesState<AudioFlowNode>(initialNodes.current);
  const [edges, setEdges, applyEdgeChanges] = useEdgesState<AudioFlowEdge>(initialEdges.current);

  useEffect(
    () => setNodes((current) => reconcileAudioFlowNodes(current, model)),
    [model, setNodes],
  );
  useEffect(
    () => setEdges((current) => reconcileAudioFlowEdges(current, model, edgeLabel)),
    [edgeLabel, model, setEdges],
  );
  const onNodesChange = useNodeChangeHandler(model, commit, applyNodeChanges, disclosureHeights);
  const layeredNodes = useMemo(
    () => applyAudioNodeLayers(nodes, openedNodeId),
    [nodes, openedNodeId],
  );
  return { applyEdgeChanges, onNodesChange, edges, nodes: layeredNodes, setEdges, setNodes };
}

function toGraphConnection(connection: Connection | Edge): AudioGraphConnection {
  return {
    sourceNodeId: connection.source,
    sourcePortId: connection.sourceHandle ?? '',
    targetNodeId: connection.target,
    targetPortId: connection.targetHandle ?? '',
  };
}

function selectedOutputPorts(
  model: AudioGraphModel,
  selectedPorts: readonly SelectedAudioPort[],
  nodeId: string,
) {
  const node = model.nodes.find((candidate) => candidate.id === nodeId);
  if (!node) return [];
  const selectedIds = new Set(
    selectedPorts
      .filter((port) => port.nodeId === nodeId && port.side === 'output')
      .map((port) => port.portId),
  );
  return node.outputPorts.filter((port) => selectedIds.has(port.id));
}

function portVerticalOffset(
  node: ConnectionLineComponentProps<AudioFlowNode>['fromNode'],
  side: AudioPortSide,
  portId: string,
  anchorPortId: string,
) {
  const handles = node.internals.handleBounds?.[side === 'input' ? 'target' : 'source'];
  const port = handles?.find((handle) => handle.id === portId);
  const anchor = handles?.find((handle) => handle.id === anchorPortId);
  if (!port || !anchor) return 0;
  // React Flow applies viewport zoom to the whole connection layer. Use measured
  // handle centers in node coordinates, including the node's current resize.
  return port.y + port.height / 2 - anchor.y - anchor.height / 2;
}

interface ConnectionPreviewPair {
  sourcePort: Pick<AudioGraphPort, 'id'>;
  targetPort?: Pick<AudioGraphPort, 'id'>;
}

function connectionPreviewPairs(
  model: AudioGraphModel,
  selectedPorts: readonly SelectedAudioPort[],
  sourceNodeId: string,
  sourceAnchorId: string,
  targetNodeId?: string,
  targetAnchorId?: string,
): ConnectionPreviewPair[] {
  const selectedSources = selectedOutputPorts(model, selectedPorts, sourceNodeId);
  const activeSources =
    selectedSources.length > 1 && selectedSources.some((port) => port.id === sourceAnchorId)
      ? selectedSources
      : [{ id: sourceAnchorId }];
  if (!targetNodeId || !targetAnchorId) {
    return activeSources.map((sourcePort) => ({ sourcePort }));
  }
  return selectedConnectionBatch(
    model,
    {
      sourceNodeId,
      sourcePortId: sourceAnchorId,
      targetNodeId,
      targetPortId: targetAnchorId,
    },
    selectedPorts,
  ).map((connection) => ({
    sourcePort: { id: connection.sourcePortId },
    targetPort: { id: connection.targetPortId },
  }));
}

/** A drag from `connection`'s source port carries the other selected outputs of that node
 * when the grabbed port is part of the selection; otherwise only the grabbed port. */
function dragConnectionRequest(
  model: AudioGraphModel,
  connection: AudioGraphConnection,
  selectedPorts: readonly SelectedAudioPort[],
): ChannelConnectionRequest {
  const selected = selectedOutputPorts(model, selectedPorts, connection.sourceNodeId);
  const batch = selected.length > 1 && selected.some((port) => port.id === connection.sourcePortId);
  return {
    sourceNodeId: connection.sourceNodeId,
    sourcePortIds: batch ? selected.map((port) => port.id) : [connection.sourcePortId],
    sourceAnchorPortId: connection.sourcePortId,
    targetNodeId: connection.targetNodeId,
    targetAnchorPortId: connection.targetPortId,
  };
}

function selectedConnectionBatch(
  model: AudioGraphModel,
  connection: AudioGraphConnection,
  selectedPorts: readonly SelectedAudioPort[],
): AudioGraphConnection[] {
  const { pairs } = mapChannelConnections(
    model,
    dragConnectionRequest(model, connection, selectedPorts),
  );
  // A target without the hovered port still previews the grabbed cable alone; committing
  // validates through planChannelConnections and creates nothing.
  return pairs.length ? pairs : [connection];
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
  // This private renderer is installed only below AudioGraphProviders.
  const selection = useContext(AudioPortSelectionContext) as AudioPortSelectionContextValue;
  const sourceAnchorId = fromHandle.id ?? '';
  const targetAnchorId = toHandle?.id ?? '';
  const previews = connectionPreviewPairs(
    selection.model,
    selection.selectedRef.current,
    fromNode.id,
    sourceAnchorId,
    toNode?.id,
    targetAnchorId,
  );

  return (
    <>
      {previews.map(({ sourcePort, targetPort }, index) => {
        const sourceOffset = portVerticalOffset(fromNode, 'output', sourcePort.id, sourceAnchorId);
        const targetOffset =
          toNode && targetPort
            ? portVerticalOffset(toNode, 'input', targetPort.id, targetAnchorId)
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
      const { accepted: connections } = planChannelConnections(
        model,
        dragConnectionRequest(model, toGraphConnection(connection), selectedPortsRef.current),
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
  disclosureHeights: Map<string, number>,
) {
  return useCallback(
    (changes: NodeChange<AudioFlowNode>[]) => {
      // Preserve XYFlow's live measurements, but do not pin the outer height:
      // the card includes temporary Options content in its rendered bounds.
      applyNodeChanges(
        changes.map((change) =>
          change.type === 'dimensions' && change.setAttributes
            ? { ...change, setAttributes: 'width' }
            : change,
        ),
      );
      const normalizedSizes = new Map(
        changes.flatMap((change) =>
          change.type === 'dimensions' && change.dimensions && change.setAttributes
            ? [
                [
                  change.id,
                  {
                    width: change.dimensions.width,
                    height: change.dimensions.height - (disclosureHeights.get(change.id) ?? 0),
                  },
                ] as const,
              ]
            : [],
        ),
      );
      let nextModel = model;
      for (const change of changes) {
        if (change.type === 'position' && change.position) {
          nextModel = moveAudioGraphNode(nextModel, change.id, change.position);
        }
        if (change.type === 'dimensions') {
          const size = normalizedSizes.get(change.id);
          if (size) nextModel = resizeAudioGraphNode(nextModel, change.id, size);
        }
      }
      commit(nextModel, 'layout');
    },
    [applyNodeChanges, commit, disclosureHeights, model],
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
  const { t } = useTranslation();
  const actionLabel = label?.trim() || t('routing.addNode');
  if (nodeMenuItems.length === 0) return null;
  return (
    <ReactFlowPanel className={`${styles.nodeToolbar} nodrag nopan nowheel`} position="top-left">
      <HierarchicalMenuButton
        buttonClassName={`${styles.addNode} nodrag nopan nowheel`}
        buttonContent={<span aria-hidden="true">+</span>}
        items={nodeMenuItems}
        label={actionLabel}
        tooltip={actionLabel}
        onClick={nodeMenu.prepareButtonPosition}
        onOpenedChange={nodeMenu.setButtonOpened}
        onSelect={addNode}
        opened={nodeMenu.buttonOpened}
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
      <Background color="var(--canvas-grid)" gap={24} size={1} variant={BackgroundVariant.Lines} />
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

function handleGraphKeyDown(
  event: ReactKeyboardEvent<HTMLElement>,
  captureProps: ReturnType<typeof useAudioPortSelection>['captureProps'],
  clearSelection: () => void,
  renameNode?: (nodeId: string) => void,
) {
  if (openContextMenuFromKeyboard(event)) return;
  if (event.key === 'F2' && event.target instanceof HTMLElement && renameNode) {
    const id = event.target.matches('.react-flow__node') ? event.target.dataset.id : undefined;
    if (id) {
      event.preventDefault();
      event.stopPropagation();
      renameNode(id);
      return;
    }
  }
  captureProps.onKeyDownCapture(event);
  if (
    event.key === 'Escape' &&
    event.target instanceof Element &&
    event.target.matches('.react-flow__node, .react-flow__edge')
  ) {
    // React Flow blurs on Escape. Clear selection while keeping the user's place.
    event.preventDefault();
    event.stopPropagation();
    clearSelection();
  }
}

function GraphContextSurface({
  children,
  contextMenu,
  nodeMenu,
  addNode,
  ariaLabel,
  captureProps,
  renameNode,
}: {
  children: ReactNode;
  contextMenu: ReturnType<typeof useGraphContextMenu>;
  nodeMenu: ReturnType<typeof useNodeMenu>;
  addNode: (id: string) => void;
  ariaLabel: string;
  captureProps: ReturnType<typeof useAudioPortSelection>['captureProps'];
  renameNode?: (nodeId: string) => void;
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
        onKeyDownCapture={(event) =>
          handleGraphKeyDown(event, captureProps, contextMenu.clearSelection, renameNode)
        }
        onPointerDownCapture={(event) => {
          // The whole canvas is the context trigger, so the menu's outside-click
          // handler cannot dismiss clicks on other nodes or ports inside it.
          if (event.button !== 2) nodeMenu.setContextOpened(false);
          captureProps.onPointerDownCapture(event);
        }}
        aria-label={ariaLabel}
        tabIndex={-1}
        className={styles.root}
        ref={nodeMenu.rootRef}
        onContextMenu={(event) => {
          contextMenu.prepare(event);
          nodeMenu.prepareContextPosition(event);
        }}
        onTouchStart={(event) => {
          // the context trigger's long-press opens without dispatching a contextmenu event.
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

function GraphFlowSurface({
  focusRoot,
  ...props
}: ReactFlowProps<AudioFlowNode, AudioFlowEdge> & { focusRoot: RefObject<HTMLElement | null> }) {
  const ariaLabelConfig = useGraphAriaLabels();
  // React Flow's deletion keys listen on the whole document. Keeping the listener mounted
  // preserves its key tracking; deletion itself proceeds only while focus is in the graph,
  // so Delete in a dialog or elsewhere never removes the selection behind it.
  const onBeforeDelete = useCallback(
    () => Promise.resolve(Boolean(focusRoot.current?.contains(document.activeElement))),
    [focusRoot],
  );
  return (
    <ReactFlow<AudioFlowNode, AudioFlowEdge>
      {...props}
      ariaLabelConfig={ariaLabelConfig}
      colorMode="dark"
      proOptions={{ hideAttribution: true }}
      connectionLineComponent={BatchConnectionLine}
      connectionMode={ConnectionMode.Strict}
      zIndexMode="manual"
      deleteKeyCode={deleteKeyCode}
      onBeforeDelete={onBeforeDelete}
      edgesReconnectable
      fitViewOptions={fitViewOptions}
      // Match the persisted layout contract; React Flow's defaults would clamp restored views.
      minZoom={0.05}
      maxZoom={8}
      multiSelectionKeyCode="Shift"
      nodeTypes={nodeTypes}
      panActivationKeyCode={platformModifier()}
      panOnDrag={panMouseButtons}
      reconnectRadius={18}
      selectionOnDrag
      snapGrid={snapGrid}
      snapToGrid
    />
  );
}

function useGraphCommit(
  model: AudioGraphModel,
  onModelChange: AudioGraphCanvasProps['onModelChange'],
) {
  return useCallback(
    (nextModel: AudioGraphModel, kind: AudioGraphChangeKind) => {
      if (nextModel !== model) onModelChange(nextModel, kind);
    },
    [model, onModelChange],
  );
}

function clearPaneOptions(clear: (value: string | null) => void, click: () => void) {
  clear(null);
  click();
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
  onNodeRenameRequest,
  onNodeEndpointChange,
  onNodeGainChange,
  onNodeParameterChange,
  onNodeParametersChange,
  onProcessorEditorRetry,
  processorEditorLabels,
  processorEditorStatuses = emptyProcessorEditorStatuses,
  renderNodeContent,
  renderNodeControls,
  viewport,
  onViewportChange,
}: AudioGraphCanvasProps) {
  const options = useState<string | null>(null);
  const disclosureHeights = useRef(new Map<string, number>()).current;
  const nodeMenu = useNodeMenu(onNodeMenuOpenedChange);
  const viewportEvents = useCanvasViewport(viewport, onViewportChange, nodeMenu);
  const commit = useGraphCommit(model, onModelChange);
  const { applyEdgeChanges, onNodesChange, edges, nodes, setEdges, setNodes } =
    useAudioFlowCollections(model, commit, disclosureHeights, options[0]);
  const selectEdges = useEdgeBoxSelection(setEdges);
  const portSelection = useAudioPortSelection(nodeMenu.rootRef, selectEdges);
  const { isValidConnection, onConnect, onReconnect, onReconnectEnd } = useConnectionHandlers(
    model,
    commit,
    portSelection.selectedRef,
    portSelection.clear,
  );
  const { onDelete, onEdgeDoubleClick } = useDeletionHandlers(model, commit);
  const { addNode, editorContext, onPaneClick } = useCanvasViewActions(
    onAddNode,
    onProcessorEditorRetry,
    processorEditorLabels,
    processorEditorStatuses,
    nodeMenu,
    portSelection.clear,
  );
  const { renaming, connecting } = useNodeRequests(onNodeRenameRequest, nodeMenu.rootRef);
  const contextMenu = useGraphContextMenu({
    ...{ model, nodes, edges, setNodes, setEdges },
    addItems: nodeMenuItems,
    labels: contextMenuLabels,
    renameNode: renaming.request,
    connectChannels: connecting.request,
    commit,
    clearPorts: portSelection.clear,
    selectPorts: portSelection.select,
    selectedPorts: portSelection.selectedRef,
    fitView: () => void nodeMenu.flowRef.current?.fitView(fitViewOptions),
  });

  return (
    <>
      <GraphContextSurface
        contextMenu={contextMenu}
        nodeMenu={nodeMenu}
        addNode={addNode}
        ariaLabel={ariaLabel}
        captureProps={portSelection.captureProps}
        renameNode={renaming.request}
      >
        <PortSelectionFeedback model={model} {...portSelection.feedback} />
        <AudioGraphProviders
          options={options}
          editor={editorContext}
          meterStore={meterStore ?? null}
          model={model}
          onEndpointChange={onNodeEndpointChange}
          onGainChange={onNodeGainChange}
          onParameterChange={onNodeParameterChange}
          onParametersChange={onNodeParametersChange}
          selectedRef={portSelection.selectedRef}
          renderNodeContent={renderNodeContent}
          renderNodeControls={renderNodeControls}
          disclosureHeights={disclosureHeights}
        >
          <GraphFlowSurface
            {...viewportEvents}
            focusRoot={nodeMenu.rootRef}
            defaultViewport={viewport}
            edges={edges}
            fitView={viewport === undefined}
            isValidConnection={isValidConnection}
            nodes={nodes}
            onConnect={onConnect}
            onDelete={onDelete}
            onEdgeDoubleClick={onEdgeDoubleClick}
            onNodeDoubleClick={renaming.doubleClick}
            onEdgesChange={applyEdgeChanges}
            onNodesChange={onNodesChange}
            onPaneClick={() => clearPaneOptions(options[1], onPaneClick)}
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
      <CanvasConnectionDialog connecting={connecting} model={model} commit={commit} />
    </>
  );
}

function CanvasConnectionDialog({
  connecting,
  model,
  commit,
}: {
  connecting: ReturnType<typeof useChannelConnectionRequests>;
  model: AudioGraphModel;
  commit: ReturnType<typeof useGraphCommit>;
}) {
  if (!connecting.current) return null;
  return (
    <ChannelConnectionDialog
      key={connecting.current.nodeId}
      model={model}
      sourceNodeId={connecting.current.nodeId}
      sourcePortIds={connecting.current.portIds}
      onCancel={connecting.close}
      onConnect={(connections) => {
        // One commit, so the whole batch is a single undo step.
        commit(connectAudioGraphBatch(model, connections), 'runtime');
        connecting.close();
      }}
    />
  );
}

/** Node-scoped dialogs share one focus contract: the node's outer group owns return focus. */
function useNodeRequests(
  onRename: AudioGraphCanvasProps['onNodeRenameRequest'],
  root: { current: HTMLElement | null },
) {
  return {
    renaming: useNodeRenameRequests(onRename, root),
    connecting: useChannelConnectionRequests(root),
  };
}

function useNodeRenameRequests(
  onRequest: AudioGraphCanvasProps['onNodeRenameRequest'],
  root: { current: HTMLElement | null },
) {
  const request = useMemo(() => {
    if (!onRequest) return undefined;
    return (id: string) => {
      focusGraphNode(root.current as HTMLElement, id);
      onRequest(id);
    };
  }, [root, onRequest]);
  return {
    request,
    doubleClick: request
      ? (event: ReactMouseEvent, node: AudioFlowNode) => renameNodeTitle(event, node.id, request)
      : undefined,
  };
}

function renameNodeTitle(event: ReactMouseEvent, id: string, rename: (nodeId: string) => void) {
  if (!(event.target instanceof Element) || !event.target.closest('[data-node-title]')) return;
  event.preventDefault();
  event.stopPropagation();
  event.target.closest<HTMLElement>('.react-flow__node')?.focus();
  rename(id);
}

function AudioFlowNodeView({ data, selected }: NodeProps<AudioFlowNode>) {
  const { t } = useTranslation();
  const { graphNode } = data;
  // AudioFlowNodeView is private to the canvas provider tree.
  const disclosureHeights = useContext(NodeDisclosureHeightsContext) as Map<string, number>;
  const onNodeGainChange = useContext(NodeGainContext);
  const onNodeEndpointChange = useContext(NodeEndpointContext);
  const { single: onNodeParameterChange, batch: onNodeParametersChange } =
    useContext(NodeParameterContext);
  const processorEditors = useContext(NodeProcessorEditorContext);
  const renderNodeContent = useContext(NodeContentContext);
  const controls = useContext(NodeControlsContext)?.(graphNode);
  const updateNodeInternals = useUpdateNodeInternals();
  const editorStatus = processorEditors?.statuses.get(graphNode.id);
  const editorWidth = editorStatus?.state === 'open' ? editorStatus.width : null;
  const layout = useAudioNodeLayout(graphNode, {
    width: editorWidth,
    height: editorStatus?.height ?? null,
  });
  useLayoutEffect(() => {
    if (layout.disclosureHeight > 0) disclosureHeights.set(graphNode.id, layout.disclosureHeight);
    else disclosureHeights.delete(graphNode.id);
    return () => {
      disclosureHeights.delete(graphNode.id);
    };
  }, [disclosureHeights, graphNode.id, layout.disclosureHeight]);
  return (
    <NodeOptionsIdContext.Provider value={graphNode.id}>
      <div
        className={styles.node}
        data-node-type={graphNode.runtime?.type}
        data-equalizer={Boolean(graphNode.appearance.equalizer) || undefined}
        data-dynamics={Boolean(graphNode.appearance.dynamics) || undefined}
        style={{
          width:
            normalizeAudioGraphNodeSize(graphNode, graphNode.size)?.width ??
            (editorWidth ? Math.min(1600, Math.max(240, editorWidth)) : undefined),
          height: layout.height,
          minHeight: layout.minHeight,
        }}
      >
        <NodePortHandles node={graphNode} geometry={layout.ports} side="input" />
        <AudioNodeCard
          {...graphNode.appearance}
          title={audioGraphNodeTitle(graphNode)}
          optionsLabel={graphNode.appearance.optionsLabel ?? t('routing.nodes.options')}
          content={renderNodeContent?.(graphNode)}
          headerActions={controls?.power}
          muteControl={controls?.mute}
          poweredOff={graphNode.runtime?.control?.enabled === false}
          faceHeight={layout.faceHeight}
          faceMode={graphNodeFaceMode(graphNode)}
          onFaceHeightChange={layout.onFaceHeightChange}
          onContentHeightChange={layout.onContentHeightChange}
          inputCount={graphNode.inputPorts.length}
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
          onParametersChange={
            onNodeParametersChange
              ? (updates) => onNodeParametersChange(graphNode.id, updates)
              : undefined
          }
          onProcessorEditorRetry={
            graphNode.appearance.hasProcessorEditor && processorEditors
              ? () => processorEditors.onRetry(graphNode.id)
              : undefined
          }
          outputCount={graphNode.outputPorts.length}
          processorEditor={processorEditorProps(
            graphNode.id,
            graphNode.appearance.hasProcessorEditor,
            processorEditors,
          )}
          selected={selected}
          showPorts={false}
        />
        <NodePortHandles node={graphNode} geometry={layout.ports} side="output" />
        <NodeResizeGrip minHeight={layout.minHeight} minWidth={graphNodeMinimumWidth(graphNode)} />
      </div>
    </NodeOptionsIdContext.Provider>
  );
}

function NodePortHandles({
  node,
  geometry,
  side,
}: {
  node: AudioGraphNode;
  geometry: Parameters<typeof audioNodePortTop>[0];
  side: AudioPortSide;
}) {
  const ports = side === 'input' ? node.inputPorts : node.outputPorts;
  return namedAudioPorts(ports).map(({ port, name, channel }, index) => (
    <AudioPortHandle
      channel={channel}
      name={name}
      top={audioNodePortTop(geometry, index, ports.length)}
      key={port.id}
      nodeId={node.id}
      port={port}
      side={side}
    />
  ));
}

function NodeResizeGrip({ minHeight, minWidth }: { minHeight: number; minWidth: number }) {
  return (
    <NodeResizeControl
      className={`${styles.resizeHandle} nodrag nopan`}
      // Keep the independent translate reset out of CSS transform lowering.
      style={{ translate: 'none' }}
      position="bottom-right"
      minWidth={minWidth}
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
    actionLabel: status.state === 'closed' ? context.labels.open : context.labels.retry,
    failureLabel: status.failure ? context.labels.failure[status.failure] : undefined,
    state: status.state,
  };
}

function AudioPortHandle({
  top,
  channel: channelNumber,
  name,
  nodeId,
  port,
  side,
}: {
  top: number;
  channel: number;
  name: string;
  nodeId: string;
  port: AudioGraphPort;
  side: 'input' | 'output';
}) {
  const isConnectionTarget = useConnection(
    (connection) =>
      connection.inProgress &&
      connection.toHandle?.nodeId === nodeId &&
      connection.toHandle.id === port.id,
  );

  return (
    <Tooltip
      kind="readout"
      nonInteractive
      label={`CH ${channelNumber}`}
      offset={7}
      opened={isConnectionTarget ? true : undefined}
      delay={isConnectionTarget ? 0 : 160}
      position={side === 'input' ? 'left' : 'right'}
    >
      {/* A named graphic: ports stay out of Tab order but expose node direction and channel. */}
      <Handle
        role="img"
        aria-label={`${name}, CH ${channelNumber}`}
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
