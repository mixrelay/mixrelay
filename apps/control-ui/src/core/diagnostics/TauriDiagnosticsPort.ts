import { invoke } from '@tauri-apps/api/core';

import type { DiagnosticsPort, SupportReportContext } from './DiagnosticsPort';

export class TauriDiagnosticsPort implements DiagnosticsPort {
  saveReport(context: SupportReportContext): Promise<void> {
    return invoke<void>('diagnostics_save_report', { context });
  }
}

/** Only the desktop app can write a native report; browser clients have no save action. */
export const localDiagnosticsPort: DiagnosticsPort | null =
  '__TAURI_INTERNALS__' in window ? new TauriDiagnosticsPort() : null;
