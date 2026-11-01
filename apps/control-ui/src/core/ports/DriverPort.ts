import type { DriverCommand, DriverSnapshot } from '../domain/driver';

/** This owner controls free devices independently of graph/audio licensing. */
export interface DriverPort {
  getSnapshot(): Promise<DriverSnapshot>;
  mutate(command: DriverCommand): Promise<DriverSnapshot>;
}
