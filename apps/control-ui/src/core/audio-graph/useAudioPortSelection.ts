import {
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  useCallback,
  useRef,
  useState,
} from 'react';

export interface SelectedAudioPort {
  nodeId: string;
  portId: string;
  side: 'input' | 'output';
}

const portSelectionDistance = 3;

function audioPortKey(port: SelectedAudioPort) {
  return `${port.side}\u0000${port.nodeId}\u0000${port.portId}`;
}

interface PortSelectionStart {
  additive: boolean;
  nodeId?: string;
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
  let nearestNode: string | undefined;
  let nearestDistance = Number.POSITIVE_INFINITY;
  for (const handle of handles ?? []) {
    if (handle.dataset.audioPortSide !== 'output') continue;
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
    if (nodeId && portId && side === 'output') {
      hits.push({ nodeId, portId, side });
      const distance =
        (bounds.left + bounds.width / 2 - start.x) ** 2 +
        (bounds.top + bounds.height / 2 - start.y) ** 2;
      if (distance < nearestDistance) {
        nearestDistance = distance;
        nearestNode = nodeId;
      }
    }
  }
  start.nodeId ??= nearestNode;
  return hits.filter((port) => port.nodeId === start.nodeId);
}

function mergeAudioPorts(
  current: readonly SelectedAudioPort[],
  added: readonly SelectedAudioPort[],
  nodeId?: string,
) {
  const sameNode = current.filter((port) => port.side === 'output' && port.nodeId === nodeId);
  const combined = new Map(sameNode.map((port) => [audioPortKey(port), port]));
  for (const port of added) combined.set(audioPortKey(port), port);
  return [...combined.values()];
}

function highlightAudioPorts(root: HTMLElement | null, selected: readonly SelectedAudioPort[]) {
  const selectedKeys = new Set(selected.map(audioPortKey));
  const handles = root?.querySelectorAll<HTMLElement>('[data-audio-port-side]');
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
}

function usePortSelectionFeedback(rootRef: { current: HTMLElement | null }) {
  const [feedback, setFeedback] = useState<{
    ports: SelectedAudioPort[];
    position: { x: number; y: number } | null;
  }>({ ports: [], position: null });
  const showFeedback = useCallback(
    (ports: SelectedAudioPort[], position: { x: number; y: number } | null) => {
      highlightAudioPorts(rootRef.current, ports);
      setFeedback((current) =>
        current.position?.x === position?.x &&
        current.position?.y === position?.y &&
        current.ports.length === ports.length &&
        current.ports.every(
          (port, index) =>
            port.nodeId === ports[index]?.nodeId &&
            port.portId === ports[index]?.portId &&
            port.side === ports[index]?.side,
        )
          ? current
          : { ports, position },
      );
    },
    [rootRef],
  );
  return [feedback, showFeedback] as const;
}

export function useAudioPortSelection(
  rootRef: { current: HTMLElement | null },
  onSelectEdges: (edgeIds: readonly string[]) => void,
) {
  const selectedRef = useRef<SelectedAudioPort[]>([]);
  const dragStartRef = useRef<PortSelectionStart | null>(null);
  const [feedback, showFeedback] = usePortSelectionFeedback(rootRef);
  const syncSelection = useCallback(
    (selected: readonly SelectedAudioPort[]) => {
      selectedRef.current = [...selected];
      showFeedback(selectedRef.current, null);
    },
    [showFeedback],
  );
  const clear = useCallback(() => syncSelection([]), [syncSelection]);
  const onPointerDownCapture = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    dragStartRef.current = portSelectionStart(event);
    if (dragStartRef.current?.additive) {
      dragStartRef.current.nodeId = selectedRef.current.find(
        (port) => port.side === 'output',
      )?.nodeId;
    }
    if (dragStartRef.current) event.currentTarget.focus({ preventScroll: true });
  }, []);
  const onPointerMoveCapture = useCallback(
    (event: ReactPointerEvent<HTMLElement>) => {
      const start = dragStartRef.current;
      if (!start || start.pointerId !== event.pointerId) return;
      const end = { x: event.clientX, y: event.clientY };
      if (Math.hypot(end.x - start.x, end.y - start.y) <= portSelectionDistance) return;
      const hits = selectedAudioPorts(rootRef.current, start, end);
      const bounds = rootRef.current?.getBoundingClientRect();
      showFeedback(
        start.additive ? mergeAudioPorts(selectedRef.current, hits, start.nodeId) : hits,
        { x: end.x - (bounds?.left ?? 0) + 12, y: end.y - (bounds?.top ?? 0) + 12 },
      );
    },
    [rootRef, showFeedback],
  );
  const onPointerUpCapture = useCallback(
    (event: ReactPointerEvent<HTMLElement>) => {
      const start = dragStartRef.current;
      if (!start || start.pointerId !== event.pointerId) return;
      dragStartRef.current = null;
      if (Math.hypot(event.clientX - start.x, event.clientY - start.y) <= portSelectionDistance) {
        syncSelection(selectedRef.current);
        return;
      }
      const end = {
        x: event.clientX,
        y: event.clientY,
      };
      const hits = selectedAudioPorts(rootRef.current, start, end);
      syncSelection(
        start.additive ? mergeAudioPorts(selectedRef.current, hits, start.nodeId) : hits,
      );
      if (start.additive) {
        onSelectEdges(selectedAudioEdgeIds(rootRef.current, selectionRectangle(start, end)));
      }
    },
    [onSelectEdges, rootRef, syncSelection],
  );
  const onPointerCancelCapture = useCallback(() => {
    dragStartRef.current = null;
    syncSelection(selectedRef.current);
  }, [syncSelection]);
  const onKeyDownCapture = useCallback(
    (event: ReactKeyboardEvent<HTMLElement>) => {
      if (event.key !== 'Escape') return;
      const previous = dragStartRef.current ? selectedRef.current : [];
      dragStartRef.current = null;
      syncSelection(previous);
    },
    [syncSelection],
  );
  return {
    captureProps: {
      onKeyDownCapture,
      onPointerCancelCapture,
      onPointerDownCapture,
      onPointerMoveCapture,
      onPointerUpCapture,
    },
    clear,
    feedback,
    selectedRef,
    select: syncSelection,
  };
}
