import {
  type ComponentProps,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';

import type {
  ControlPort,
  GraphCatalogAction,
  RuntimeGraphDocument,
} from '../core/control-port/types';
import { ActionButton } from '../core/ui/ActionButton';
import {
  hasPlatformModifier,
  isNativeTextEntryTarget,
  platformModifier,
} from '../core/ui/platformShortcut';
import { AudioSessionControl } from '../features/audio-session/AudioSessionControl';
import { EnginePowerControl } from '../features/audio-session/EnginePowerControl';
import { useAudioStatus } from '../features/audio-session/useAudioStatus';
import type { DriverSession } from '../features/driver-status/useDriverSession';
import { GraphCreateDialog } from '../features/graphs/GraphCreateDialog';
import { GraphList } from '../features/graphs/GraphList';
import { useCatalogTitle, useGraphCatalog } from '../features/graphs/useGraphCatalog';
import { WorkspaceFileMenu } from '../features/graphs/WorkspaceFileMenu';
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
      if (
        event.defaultPrevented ||
        event.altKey ||
        isNativeTextEntryTarget(event.target) ||
        !hasPlatformModifier(event) ||
        !event.shiftKey ||
        event.key.toLowerCase() !== 'n'
      ) {
        return;
      }
      event.preventDefault();
      if (enabled) open();
    };
    window.addEventListener('keydown', handle);
    return () => window.removeEventListener('keydown', handle);
  }, [enabled, open]);
}

function workspaceSessionDocument(
  sessionDocument: RuntimeGraphDocument | undefined,
  catalogRevision: number | undefined,
  fallback: RuntimeGraphDocument | undefined,
) {
  return sessionDocument && sessionDocument.revision >= (catalogRevision ?? 0)
    ? sessionDocument
    : fallback;
}

function routingWorkspaceRevisionProps(
  status: ReturnType<typeof useAudioStatus>,
  catalog: ReturnType<typeof useGraphCatalog>['catalog'],
) {
  return {
    failedProcessors: status?.failedProcessors,
    catalogRevision: catalog?.catalogRevision,
  };
}

export function StudioScreen({
  controlPort,
  applicationActions,
  driverSession,
}: {
  controlPort: ControlPort;
  applicationActions?: ReactNode;
  driverSession?: DriverSession;
}) {
  const { t } = useTranslation();
  const [statusRefresh, setStatusRefresh] = useState(0);
  const status = useAudioStatus(controlPort, statusRefresh);
  const collection = useGraphCatalog(controlPort);
  const catalogErrorText = useCatalogError(collection.error);
  const { catalog, busy: catalogBusy, uncertain: catalogUncertain } = collection;
  const [sessionBusy, setSessionBusy] = useState(false);
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const ownedSession = useOwnedSessionDocument(controlPort, collection.document);
  const [sessionDocument, setSessionDocument] = ownedSession;
  const { isCreatingGraph, setIsCreatingGraph, openNewGraph } = useGraphCreation(catalog?.activeId);
  const busy = catalogBusy || sessionBusy || workspaceBusy || catalogUncertain;
  const title = useCatalogTitle(collection, busy, t('studio.untitledGraph'));
  const controlsDisabled = busy || title.unsaved;
  const onSessionApplied = (document: RuntimeGraphDocument) => {
    setSessionDocument(document);
    setStatusRefresh((value) => value + 1);
  };
  const mutateCatalog = (action: GraphCatalogAction) => {
    if (!busy) return collection.mutate(action);
  };
  const documentForWorkspace = workspaceSessionDocument(
    sessionDocument,
    catalog?.document.revision,
    collection.document,
  );

  return (
    <StudioFrame
      applicationActions={applicationActions}
      collection={collection}
      disabled={controlsDisabled}
      onCreate={openNewGraph}
      onSelect={(id) => void mutateCatalog({ action: 'select', id })}
    >
      <section
        className={styles.workspace}
        aria-label={t('studio.workspace')}
        inert={sessionBusy || catalogBusy || catalogUncertain}
      >
        <RoutingWorkspace
          headerActions={
            <WorkspaceFileMenu
              controlPort={controlPort}
              disabled={controlsDisabled}
              run={collection.workspaceFile}
            />
          }
          deviceDriverAction={{
            settings: driverSession?.snapshot ? {
              ...driverSession.snapshot.settings,
              canCreate: driverSession.allowMultiDevice && driverSession.snapshot.settings.canCreate,
            } : undefined,
            enabled: driverSession?.snapshot?.state === 'ready' && !driverSession.busy,
            error: driverSession?.error ? { endpointId: driverSession.endpointId, message: driverSession.error } : null,
            change: (endpointId, update) => { void driverSession?.changeDevice(endpointId, update); },
            create: async (spec) => {
              const snapshot = await driverSession?.mutate({ action: 'create', spec });
              return snapshot?.settings.devices.find((device) => device.spec.id === spec.id)?.spec ?? null;
            },
          }}
          key={`${catalog?.activeId ?? 'engine'}:${collection.documentGeneration}`}
          graphId={catalog?.activeId}
          interactionDisabled={catalogBusy || catalogUncertain || sessionBusy}
          audioSession={status?.audioSession}
          processorTimings={status?.processorTimings}
          {...routingWorkspaceRevisionProps(status, catalog)}
          sessionDocument={documentForWorkspace}
          controlPort={controlPort}
          onBusyChange={setWorkspaceBusy}
          onTitleChange={title.setDraft}
          title={catalog ? title.draft : undefined}
        />
      </section>

      <StudioStatusBar
        disabled={controlsDisabled}
        status={status}
        controlPort={controlPort}
        onApplied={onSessionApplied}
        onBusyChange={setSessionBusy}
      />

      {isCreatingGraph ? (
        <GraphCreateDialog
          busy={controlsDisabled}
          creating={catalogBusy}
          error={catalogErrorText}
          onCancel={() => setIsCreatingGraph(false)}
          onCreate={(name) => void mutateCatalog({ action: 'create', name })}
          suggestedName={t('studio.untitledGraphNumbered', {
            count: (catalog?.graphs.length ?? 0) + 1,
          })}
        />
      ) : null}
    </StudioFrame>
  );
}

function useGraphCreation(activeId: string | undefined) {
  const [isCreatingGraph, setIsCreatingGraph] = useState(false);
  useEffect(() => {
    if (activeId) setIsCreatingGraph(false);
  }, [activeId]);
  const openNewGraph = useCallback(() => setIsCreatingGraph(true), []);
  useNewGraphShortcut(Boolean(activeId), openNewGraph);
  return { isCreatingGraph, setIsCreatingGraph, openNewGraph };
}

function StudioFrame({
  children,
  applicationActions,
  ...sidebar
}: Omit<ComponentProps<typeof GraphSidebar>, 'hidden'> & {
  children: ReactNode;
  applicationActions?: ReactNode;
}) {
  const { t } = useTranslation();
  const [sidebarOpen, setSidebarOpen] = useState(true);
  return (
    <main className={styles.shell} data-sidebar-open={sidebarOpen}>
      <header className={styles.applicationHeader}>
        <h1>MixRelay</h1>
        <span className={styles.applicationActions}>{applicationActions}</span>
      </header>
      <GraphSidebar hidden={!sidebarOpen} {...sidebar} />
      <button
        className={sidebarOpen ? styles.sidebarBoundary : styles.sidebarReopen}
        type="button"
        aria-label={t('studio.graphs')}
        aria-expanded={sidebarOpen}
        aria-controls="graph-sidebar"
        onClick={() => setSidebarOpen((open) => !open)}
      >
        {!sidebarOpen ? (
          <svg aria-hidden="true" viewBox="0 0 16 16">
            <path d="m6 4 4 4-4 4" />
          </svg>
        ) : null}
      </button>
      {children}
    </main>
  );
}

function StudioStatusBar(props: ComponentProps<typeof AudioSessionControl>) {
  return (
    <footer className={styles.statusBar}>
      <AudioSessionControl {...props} />
      <EnginePowerControl {...props} />
    </footer>
  );
}

function GraphSidebar({
  hidden,
  collection,
  disabled,
  onCreate,
  onSelect,
}: {
  hidden: boolean;
  collection: ReturnType<typeof useGraphCatalog>;
  disabled: boolean;
  onCreate: () => void;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  const { catalog, busy: catalogBusy, error: catalogError, reload: reloadCatalog } = collection;
  const catalogErrorText = useCatalogError(catalogError);
  return (
    <aside
      id="graph-sidebar"
      className={styles.sidebar}
      aria-label={t('studio.graphs')}
      hidden={hidden}
    >
      <nav className={styles.graphList} aria-label={t('studio.graphs')}>
        {catalog ? (
          <GraphList
            catalog={catalog}
            disabled={disabled}
            busy={catalogBusy}
            error={catalogErrorText}
            onSelect={onSelect}
            onDelete={(id) => {
              if (!disabled) void collection.mutate({ action: 'delete', id });
            }}
          />
        ) : null}
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
        <div className={styles.sidebarFooter}>
          <button
            type="button"
            aria-keyshortcuts={`${platformModifier()}+Shift+N`}
            className={`${styles.graph} ${styles.newGraph}`}
            disabled={!catalog}
            onClick={onCreate}
          >
            <span className={styles.newGraphIcon} aria-hidden="true">
              +
            </span>
            <span className={styles.graphName}>{t('studio.newGraph')}</span>
          </button>
        </div>
      </nav>
    </aside>
  );
}
