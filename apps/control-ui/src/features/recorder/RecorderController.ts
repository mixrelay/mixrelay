import {
  type ControlPort,
  ControlPortError,
  type RecorderRequest,
  type RecorderStatus,
} from '../../core/control-port/types';

interface RecorderSnapshot {
  status: RecorderStatus | null;
  busy: boolean;
  error: string | null;
}

/** One graph/node owner serializes reads and commands; disposed owners never publish late replies. */
export class RecorderController {
  private snapshot: RecorderSnapshot = { status: null, busy: false, error: null };
  private readonly listeners = new Set<() => void>();
  private active = false;
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private tail: Promise<void> = Promise.resolve();
  private commandError: string | null = null;

  public constructor(
    private readonly port: ControlPort,
    private readonly graphId: string,
    private readonly nodeId: string,
  ) {}

  public readonly getSnapshot = () => this.snapshot;
  public readonly subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  public start(): void {
    if (this.active) return;
    this.generation += 1;
    this.active = true;
    this.publish({ ...this.snapshot, busy: false });
    this.refresh();
  }

  public dispose(): void {
    this.generation += 1;
    this.active = false;
    clearTimeout(this.timer);
  }

  public readonly command = (request: RecorderRequest, expectedRevision: number): void => {
    if (!this.active || this.snapshot.busy) return;
    clearTimeout(this.timer);
    this.commandError = null;
    this.publish({ ...this.snapshot, busy: true, error: null });
    const generation = this.generation;
    this.tail = this.tail.then(async () => {
      if (!this.active || generation !== this.generation) return;
      try {
        const status = await this.port.controlRecorder(
          this.graphId,
          this.nodeId,
          request,
          expectedRevision,
        );
        if (generation !== this.generation) return;
        this.publish({ status, busy: false, error: status.error });
      } catch (error) {
        if (generation !== this.generation) return;
        this.commandError = recorderError(error);
        this.publish({ ...this.snapshot, busy: false, error: this.commandError });
      }
      this.schedule();
    });
  };

  private refresh(): void {
    const generation = this.generation;
    this.tail = this.tail.then(async () => {
      if (!this.active || generation !== this.generation || this.snapshot.busy) return;
      try {
        const status = await this.port.recorderStatus(this.graphId, this.nodeId);
        if (generation !== this.generation) return;
        this.publish({ ...this.snapshot, status, error: this.commandError ?? status.error });
      } catch (error) {
        if (generation !== this.generation) return;
        this.publish({ ...this.snapshot, error: this.commandError ?? recorderError(error) });
      }
      this.schedule();
    });
  }

  private schedule(): void {
    clearTimeout(this.timer);
    if (this.active)
      this.timer = setTimeout(
        () => this.refresh(),
        this.snapshot.status?.state === 'recording' ? 100 : 1000,
      );
  }

  private publish(snapshot: RecorderSnapshot): void {
    if (!this.active) return;
    this.snapshot = snapshot;
    for (const listener of this.listeners) listener();
  }
}

export function recorderError(error: unknown): string {
  if (error instanceof ControlPortError) {
    const parameters = error.body.parameters;
    if (
      typeof parameters === 'object' &&
      parameters !== null &&
      'reason' in parameters &&
      typeof parameters.reason === 'string'
    )
      return parameters.reason;
  }
  return error instanceof Error ? error.message : String(error);
}

export function recordingDuration(status: RecorderStatus | null): string {
  const milliseconds = status?.sampleRate
    ? Math.floor((status.frames * 1000) / status.sampleRate)
    : 0;
  const seconds = Math.floor(milliseconds / 1000);
  const clock = [Math.floor(seconds / 3600), Math.floor(seconds / 60) % 60, seconds % 60]
    .map((part) => String(part).padStart(2, '0'))
    .join(':');
  return `${clock}.${String(milliseconds % 1000).padStart(3, '0')}`;
}

export function absoluteRecordingPath(path: string): boolean {
  return path.startsWith('/') || /^[A-Za-z]:[\\/]/u.test(path) || /^\\\\[^\\]+\\[^\\]+/u.test(path);
}
