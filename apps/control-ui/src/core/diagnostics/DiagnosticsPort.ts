import type { ExternalProcessorDiagnostic } from '../domain/control/processors';

/** Editor facts added to the native support report. Codes and counts only: no names or paths. */
export interface SupportReportContext {
  problems: string[];
  processorScan: {
    format: ExternalProcessorDiagnostic['format'];
    stage: ExternalProcessorDiagnostic['stage'];
    count: number;
  }[];
}

/** The native side opens the save dialog, collects runtime facts and writes the file. */
export interface DiagnosticsPort {
  saveReport(context: SupportReportContext): Promise<void>;
}

/** Counts unique scan failures by format and stage, dropping plug-in names and messages. */
export function scanFailureCounts(
  diagnostics: readonly ExternalProcessorDiagnostic[],
): SupportReportContext['processorScan'] {
  const unique = new Map(diagnostics.map((item) => [JSON.stringify(item), item]));
  const counts = new Map<string, SupportReportContext['processorScan'][number]>();
  for (const { format, stage } of unique.values()) {
    const key = `${format}:${stage}`;
    const entry = counts.get(key) ?? { format, stage, count: 0 };
    entry.count += 1;
    counts.set(key, entry);
  }
  return [...counts.values()];
}
