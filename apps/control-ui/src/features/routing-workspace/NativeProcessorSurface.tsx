import { type PointerEvent, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type {
  ControlPort,
  ProcessorEditorFrame,
  ProcessorEditorInput,
} from '../../core/control-port/types';
import styles from './NativeProcessorSurface.module.css';

type SurfacePort = Pick<ControlPort, 'getProcessorEditorFrame' | 'sendProcessorEditorInput'>;

/** Match object-fit: contain, including the centered letterbox around the native image. */
export function editorImagePoint(
  image: Pick<ProcessorEditorFrame, 'width' | 'height'>,
  bounds: Pick<DOMRect, 'left' | 'top' | 'width' | 'height'>,
  pointer: { clientX: number; clientY: number },
  clampOutside: boolean,
) {
  if (bounds.width <= 0 || bounds.height <= 0) return null;
  const scale = Math.min(bounds.width / image.width, bounds.height / image.height);
  const left = bounds.left + (bounds.width - image.width * scale) / 2;
  const top = bounds.top + (bounds.height - image.height * scale) / 2;
  const x = (pointer.clientX - left) / scale;
  const y = (pointer.clientY - top) / scale;
  if (!clampOutside && (x < 0 || y < 0 || x > image.width || y > image.height)) return null;
  return { x: Math.max(0, Math.min(image.width, x)), y: Math.max(0, Math.min(image.height, y)) };
}

/** One in-flight input and a bounded ordered tail; movement never displaces mouse-up. */
export class EditorPointerQueue {
  private pending: ProcessorEditorInput[] = [];
  private sending = false;
  private closed = false;
  private frame: ProcessorEditorFrame | null = null;
  constructor(
    private port: SurfacePort,
    private nodeId: string,
    private failed: () => void,
  ) {}
  image(frame: ProcessorEditorFrame) {
    this.frame = frame;
  }
  push(kind: ProcessorEditorInput['kind'], x = 0, y = 0, modifiers = 0) {
    if (!this.frame || this.closed) return;
    const input = {
      instanceId: this.frame.instanceId,
      session: this.frame.session,
      kind,
      x,
      y,
      modifiers,
    };
    const last = this.pending.at(-1);
    if (kind === 'move' && last?.kind === 'move') this.pending[this.pending.length - 1] = input;
    else if (this.pending.length >= 16) {
      this.pending = [{ ...input, kind: 'cancel' }];
      this.failed();
    } else this.pending.push(input);
    void this.drain();
  }
  private async drain() {
    if (this.sending) return;
    this.sending = true;
    while (this.pending.length) {
      const input = this.pending.shift();
      if (!input) break;
      try {
        await this.port.sendProcessorEditorInput(this.nodeId, input);
      } catch {
        if (!this.closed) this.failed();
        this.pending = [];
        await this.cancelFailedInput(input);
      }
    }
    this.sending = false;
  }
  private async cancelFailedInput(input: ProcessorEditorInput) {
    if (input.kind === 'cancel') return;
    try {
      await this.port.sendProcessorEditorInput(this.nodeId, { ...input, kind: 'cancel' });
    } catch {
      /* Destruction releases the native gesture when the worker no longer exists. */
    }
  }
  dispose() {
    this.pending = [];
    this.push('cancel');
    this.closed = true;
  }
}

export function NativeProcessorSurface({
  port,
  nodeId,
  title,
}: {
  port: SurfacePort;
  nodeId: string;
  title: string;
}) {
  const { t } = useTranslation();
  const [frame, setFrame] = useState<ProcessorEditorFrame | null>(null);
  const [failed, setFailed] = useState(false);
  const queue = useRef<EditorPointerQueue | null>(null);
  const activePointer = useRef<number | null>(null);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const pointer = new EditorPointerQueue(port, nodeId, () => setFailed(true));
    queue.current = pointer;
    setFrame(null);
    const poll = async () => {
      try {
        const image = await port.getProcessorEditorFrame(nodeId);
        if (disposed) return;
        validateFrame(image);
        pointer.image(image);
        setFrame(image);
        setFailed(false);
      } catch {
        if (!disposed) setFailed(true);
      }
      if (!disposed) timer = setTimeout(() => void poll(), 33);
    };
    void poll();
    const blur = () => pointer.push('cancel');
    window.addEventListener('blur', blur);
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
      window.removeEventListener('blur', blur);
      pointer.dispose();
      queue.current = null;
    };
  }, [port, nodeId]);
  const send = (event: PointerEvent<HTMLDivElement>, kind: ProcessorEditorInput['kind']) => {
    if (!frame || (kind === 'down' && event.button !== 0)) return;
    if (kind !== 'down' && activePointer.current !== event.pointerId) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const point = editorImagePoint(frame, rect, event, kind !== 'down');
    if (!point) return;
    event.stopPropagation();
    event.preventDefault();
    if (kind === 'down') {
      activePointer.current = event.pointerId;
      event.currentTarget.setPointerCapture(event.pointerId);
    }
    if (kind === 'up') activePointer.current = null;
    queue.current?.push(
      kind,
      point.x,
      point.y,
      Number(event.shiftKey) |
        (Number(event.ctrlKey) << 1) |
        (Number(event.altKey) << 2) |
        (Number(event.metaKey) << 3),
    );
    releaseCompletedPointer(event, kind);
  };
  const cancel = () => {
    if (activePointer.current === null) return;
    activePointer.current = null;
    queue.current?.push('cancel');
  };
  return (
    <div
      className={`${styles.root} nodrag nopan nowheel`}
      style={frame ? { aspectRatio: `${frame.width} / ${frame.height}` } : undefined}
    >
      <div
        role="application"
        aria-label={title}
        className={styles.surface}
        onPointerDown={(event) => send(event, 'down')}
        onPointerMove={(event) => send(event, 'move')}
        onPointerUp={(event) => send(event, 'up')}
        onPointerCancel={cancel}
        onLostPointerCapture={cancel}
        onContextMenu={(event) => event.preventDefault()}
      >
        {frame ? (
          <img alt={title} draggable={false} src={`data:image/png;base64,${frame.pngBase64}`} />
        ) : (
          <div className={styles.placeholder}>{t('routing.processorEditor.states.opening')}</div>
        )}
      </div>
      {failed && (
        <span role="status" className={styles.failure}>
          {t('routing.processorEditor.states.failed')}
        </span>
      )}
    </div>
  );
}

function validateFrame(image: ProcessorEditorFrame) {
  if (
    !Number.isInteger(image.width) ||
    !Number.isInteger(image.height) ||
    image.width <= 0 ||
    image.height <= 0 ||
    image.width * image.height > 4_000_000 ||
    image.pngBase64.length > 4_000_000
  ) {
    throw new Error('Invalid native editor image');
  }
}

function releaseCompletedPointer(
  event: PointerEvent<HTMLDivElement>,
  kind: ProcessorEditorInput['kind'],
) {
  if (kind === 'up' && event.currentTarget.hasPointerCapture(event.pointerId)) {
    event.currentTarget.releasePointerCapture(event.pointerId);
  }
}
