import type { ControlPort, ToneNariStatus } from './types';

export class StorybookMockControlPort implements ControlPort {
  public constructor(private readonly status: ToneNariStatus) {}

  public async getStatus(): Promise<ToneNariStatus> {
    return this.status;
  }
}
