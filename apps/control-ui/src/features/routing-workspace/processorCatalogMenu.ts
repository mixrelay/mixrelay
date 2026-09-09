import type { TFunction } from 'i18next';
import type { AudioGraphNodeMenuItem } from '../../core/audio-graph/AudioGraphCanvas';
import type { ExternalProcessorDescriptor } from '../../core/control-port/types';
import type { useProcessorCatalog } from './useProcessorCatalog';

/** Stable leaf identity shared by menu construction and processor insertion. */
export function processorItemId(processor: ExternalProcessorDescriptor): string {
  return `processor:${processor.typeId}`;
}

/** Group discovered plug-ins into format submenus without changing leaf actions. */
export function createProcessorMenuItems(
  processors: readonly ExternalProcessorDescriptor[],
  t: TFunction,
  emptyLabel: string,
): AudioGraphNodeMenuItem[] {
  if (!processors.length) return [{ disabled: true, id: 'no-processors', label: emptyLabel }];
  const order: ExternalProcessorDescriptor['format'][] = ['au', 'auv3', 'vst3', 'clap', 'lv2'];
  return order.flatMap((format) => {
    const matches = processors.filter((processor) => processor.format === format);
    if (!matches.length) return [];
    return [
      {
        id: `processor-format:${format}`,
        label: t(`routing.groups.${format}`),
        children: matches
          .sort((a, b) => a.displayName.localeCompare(b.displayName))
          .map((processor) => ({
            description: [
              processor.vendor,
              `${processor.inputChannels}→${processor.outputChannels} CH`,
            ]
              .filter(Boolean)
              .join(' · '),
            id: processorItemId(processor),
            label: processor.displayName,
          })),
      },
    ];
  });
}

/** An empty list is authoritative only after discovery finished without an error. */
export function processorCatalogPlaceholder(
  catalog: Pick<ReturnType<typeof useProcessorCatalog>, 'busy' | 'failure' | 'diagnostics'>,
) {
  if (catalog.busy) return 'routing.processorControl.scanning';
  if (catalog.failure || catalog.diagnostics.length) return 'routing.processorControl.incomplete';
  return 'routing.noProcessors';
}
