import { commandFor } from './controls.mjs';

/** Owns one observed state and one in-flight write; never queues or replays button intent. */
export class ControlSession {
  snapshot;
  observedAt = 0;
  #reading;
  #writing = false;
  #generation = 0;

  constructor(exchange, publish, now = Date.now) {
    this.exchange = exchange;
    this.publish = publish;
    this.now = now;
  }

  refresh() {
    if (this.#writing) return Promise.resolve();
    if (this.#reading) return this.#reading;
    const generation = this.#generation;
    this.#reading = this.#read(generation).finally(() => {
      this.#reading = undefined;
    });
    return this.#reading;
  }

  async #read(generation) {
    try {
      const snapshot = await this.exchange({ type: 'snapshot' }, undefined, 4000);
      if (snapshot.protocolVersion !== 1) throw new Error('Unsupported MixRelay control protocol');
      if (generation !== this.#generation) return;
      this.snapshot = snapshot;
      this.observedAt = this.now();
    } catch {
      if (generation !== this.#generation) return;
      this.snapshot = undefined;
      this.observedAt = 0;
    }
    await this.publish();
  }

  async control(settings, operation, ticks) {
    return this.#send(commandFor(this.snapshot, settings, operation, ticks));
  }

  async setGain(settings, gainDb) {
    if (!Number.isFinite(gainDb) || gainDb < -60 || gainDb > 12)
      throw new Error('Invalid absolute gain');
    const command = commandFor(this.snapshot, settings, 'gain_up', 0);
    command.request.gainDb = gainDb;
    return this.#send(command);
  }

  async #send(command) {
    if (this.#writing || this.now() - this.observedAt > 2500)
      throw new Error('Control state is busy or stale');
    this.#writing = true;
    // A concurrent read may finish, but cannot publish a pre-command state afterwards.
    this.#generation += 1;
    try {
      return await this.exchange(command);
    } finally {
      // Rendering failure must not retain the command lock or replay an accepted write.
      await Promise.allSettled([this.#reading]);
      this.#writing = false;
      await this.refresh();
    }
  }
}
