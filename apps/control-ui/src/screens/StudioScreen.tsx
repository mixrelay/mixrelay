import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import type {
  ControlPort,
  GraphCatalogAction,
  RuntimeGraphDocument,
} from '../core/control-port/types';
import { ActionButton } from '../core/ui/ActionButton';
import { ShortcutTooltip } from '../core/ui/ShortcutTooltip';
import { StatusIndicator } from '../core/ui/StatusIndicator';
import { WorkspaceHeader } from '../core/ui/WorkspaceHeader';
import { AudioSessionControl } from '../features/audio-session/AudioSessionControl';
import { getEnginePresentation } from '../features/audio-session/presentation';
import { useAudioStatus } from '../features/audio-session/useAudioStatus';
import { useVirtualEndpointRate } from '../features/audio-session/useVirtualEndpointRate';
import { GraphCreateDialog } from '../features/graphs/GraphCreateDialog';
import { useCatalogTitle, useGraphCatalog } from '../features/graphs/useGraphCatalog';
import { RoutingWorkspace } from '../features/routing-workspace/RoutingWorkspace';
import styles from './StudioScreen.module.css';

function useCatalogError(error: string | null) {
  const { t } = useTranslation();
  if (!error) return null;
  if (error === 'graph_catalog_unavailable') return t('studio.catalogUnavailable');
  return t(`routing.errors.${error}`, { defaultValue: t('routing.errors.control_request_failed') });
}

function useOwnedSessionDocument(controlPort: ControlPort, catalogDocument?: RuntimeGraphDocument) {
  const owner = useRef({ controlPort, catalogDocument });
  owner.current = { controlPort, catalogDocument };
  const [saved, setSaved] = useState<{
    controlPort: ControlPort;
    catalogDocument?: RuntimeGraphDocument;
    document: RuntimeGraphDocument;
  }>();
  const setDocument = useCallback(
    (document: RuntimeGraphDocument) => {
      // Graph selection can keep the same audio revision. A previous view's
      // completion must not replace the newly selected document, even for one render.
      if (
        owner.current.controlPort === controlPort &&
        owner.current.catalogDocument === catalogDocument
      )
        setSaved({ controlPort, catalogDocument, document });
    },
    [controlPort, catalogDocument],
  );
  return [
    saved?.controlPort === controlPort && saved.catalogDocument === catalogDocument
      ? saved.document
      : catalogDocument,
    setDocument,
  ] as const;
}

function useNewGraphShortcut(enabled: boolean, open: () => void) {
  useEffect(() => {
    const handle = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'n') {
        event.preventDefault();
        if (enabled) open();
      }
    };
    window.addEventListener('keydown', handle);
    return () => window.removeEventListener('keydown', handle);
  }, [enabled, open]);
}

export function StudioScreen({ controlPort }: { controlPort: ControlPort }) {
  const { t } = useTranslation();
  const [statusRefresh, setStatusRefresh] = useState(0);
  const status = useAudioStatus(controlPort, statusRefresh);
  const collection = useGraphCatalog(controlPort);
  const catalogErrorText = useCatalogError(collection.error);
  const { catalog, busy: catalogBusy, uncertain: catalogUncertain } = collection;
  const [sessionBusy, setSessionBusy] = useState(false);
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const [sessionDocument, setSessionDocument] = useOwnedSessionDocument(
    controlPort,
    collection.document,
  );
  const [isCreatingGraph, setIsCreatingGraph] = useState(false);
  const busy = catalogBusy || sessionBusy || workspaceBusy || catalogUncertain;
  const title = useCatalogTitle(collection, busy, t('studio.untitledGraph'));
  const unsavedTitle = title.unsaved;
  const deviceRate = useVirtualEndpointRate({
    controlPort,
    settings: status?.audioSession?.virtualEndpointSettings,
    enabled: !busy && !unsavedTitle,
    onBusyChange: setSessionBusy,
    onApplied: (document) => {
      setSessionDocument(document);
      setStatusRefresh((value) => value + 1);
    },
  });
  const mutateCatalog = (action: GraphCatalogAction) => {
    if (!busy) return collection.mutate(action);
  };
  const engine = getEnginePresentation(status, t);

  useEffect(() => {
    if (catalog?.activeId) setIsCreatingGraph(false);
  }, [catalog?.activeId]);
  const openNewGraph = useCallback(() => setIsCreatingGraph(true), []);
  useNewGraphShortcut(Boolean(catalog), openNewGraph);

  return (
    <main className={styles.shell}>
      <GraphSidebar
        collection={collection}
        disabled={busy || unsavedTitle}
        onCreate={() => setIsCreatingGraph(true)}
        onSelect={(id) => void mutateCatalog({ action: 'select', id })}
      />

      <section
        className={styles.workspace}
        aria-label={t('studio.workspace')}
        inert={sessionBusy || catalogBusy || catalogUncertain}
      >
        <RoutingWorkspace
          deviceRateAction={{
            change: deviceRate.change,
            error: deviceRate.error,
            enabled: deviceRate.available && !busy && !unsavedTitle,
          }}
          key={catalog?.activeId ?? 'engine'}
          graphId={catalog?.activeId}
          interactionDisabled={catalogBusy || catalogUncertain || sessionBusy}
          audioSession={status?.audioSession}
          sessionDocument={
            sessionDocument && sessionDocument.revision >= (catalog?.document.revision ?? 0)
              ? sessionDocument
              : collection.document
          }
          controlPort={controlPort}
          onBusyChange={setWorkspaceBusy}
          onTitleChange={(name) => {
            title.setDraft(name);
          }}
          title={catalog ? title.draft : undefined}
        />
      </section>

      <footer className={styles.statusBar}>
        <AudioSessionControl
          disabled={busy || unsavedTitle}
          status={status}
          controlPort={controlPort}
          onApplied={setSessionDocument}
          onBusyChange={setSessionBusy}
        />
        <span>{t('studio.engineShort')}</span>
        <StatusIndicator className={styles.engineDot} label={engine.label} tone={engine.tone} />
      </footer>

      {isCreatingGraph ? (
        <GraphCreateDialog
          busy={busy || unsavedTitle}
          creating={catalogBusy}
          error={catalogErrorText}
          onCancel={() => setIsCreatingGraph(false)}
          onCreate={(name) => void mutateCatalog({ action: 'create', name })}
          suggestedName={t('studio.untitledGraphNumbered', {
            count: (catalog?.graphs.length ?? 0) + 1,
          })}
        />
      ) : null}
    </main>
  );
}

function GraphSidebar({
  collection,
  disabled,
  onCreate,
  onSelect,
}: {
  collection: ReturnType<typeof useGraphCatalog>;
  disabled: boolean;
  onCreate: () => void;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  const { catalog, busy: catalogBusy, error: catalogError, reload: reloadCatalog } = collection;
  const catalogErrorText = useCatalogError(catalogError);
  return (
    <aside className={styles.sidebar} aria-label={t('studio.graphs')}>
      <WorkspaceHeader className={styles.sidebarSectionHeader}>
        <h1>{t('studio.graphs')}</h1>
      </WorkspaceHeader>

      <nav className={styles.graphList} aria-label={t('studio.graphs')}>
        {catalog?.graphs.map((graph) => {
          const selected = graph.id === catalog?.activeId;
          return (
            <button
              aria-current={selected ? 'page' : undefined}
              className={styles.graph}
              key={graph.id}
              disabled={disabled || selected}
              onClick={() => onSelect(graph.id)}
              type="button"
            >
              <span className={styles.graphName}>{graph.name}</span>
            </button>
          );
        })}
        {catalogError ? (
          <div role="alert">
            <p>{catalogErrorText}</p>
            <ActionButton
              disabled={catalogBusy}
              onClick={() => void reloadCatalog()}
              variant="subtle"
            >
              {t('studio.reloadGraphs')}
            </ActionButton>
          </div>
        ) : null}
        {catalog?.persistenceError ? <p role="alert">{t('studio.graphSaveFailed')}</p> : null}
      </nav>

      <footer className={styles.sidebarFooter}>
        <ShortcutTooltip shortcut="Mod+N">
          <ActionButton
            className={styles.newGraph}
            fullWidth
            disabled={!catalog}
            leftSection={
              <span className={styles.newGraphIcon} aria-hidden="true">
                +
              </span>
            }
            onClick={onCreate}
            variant="transparent"
          >
            {t('studio.newGraph')}
          </ActionButton>
        </ShortcutTooltip>
      </footer>
    </aside>
  );
}
