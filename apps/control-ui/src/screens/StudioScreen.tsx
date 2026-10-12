import {
  type ComponentProps,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import type { ControlPort, RuntimeGraphDocument } from '../core/control-port/types';
import { usePaidFeatures } from '../core/licensing/FeatureAccess';
import { ActionButton } from '../core/ui/ActionButton';
import { ApplicationStatusBar } from '../core/ui/ApplicationStatusBar';
import { ProductHeading } from '../core/ui/BrandIdentity';
import { IconActionButton } from '../core/ui/IconActionButton';
import {
  hasPlatformModifier,
  isNativeTextEntryTarget,
  platformModifier,
} from '../core/ui/platformShortcut';
import { SidebarToggle } from '../core/ui/SidebarToggle';
import { ThemePicker } from '../core/ui/ThemePicker';
import { AudioPauseControl } from '../features/audio-session/AudioPauseControl';
import { AudioSessionNotices } from '../features/audio-session/AudioSessionNotices';
import { useAudioStatus } from '../features/audio-session/useAudioStatus';
import { useDeviceBufferControl } from '../features/audio-session/useDeviceBufferControl';
import { DriverInstallNotice } from '../features/driver-status/DriverInstallNotice';
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

function routingWorkspaceRuntimeProps(
  status: ReturnType<typeof useAudioStatus>,
  catalog: ReturnType<typeof useGraphCatalog>['catalog'],
) {
  return {
    failedProcessors: status?.failedProcessors,
    catalogRevision: catalog?.catalogRevision,
    playbackPaused: status?.enginePower?.enabled === false || status?.enginePower?.transitioning,
  };
}

export function StudioScreen({
  controlPort,
  applicationActions,
  applicationInfo,
  driverSession,
  onInstallDriver,
}: {
  controlPort: ControlPort;
  applicationActions?: ReactNode;
  applicationInfo?: ReactNode;
  driverSession?: DriverSession;
  onInstallDriver?: () => void;
}) {
  const { t } = useTranslation();
  const paid = usePaidFeatures();
  const [statusRefresh, setStatusRefresh] = useState(0);
  const status = useAudioStatus(controlPort, statusRefresh);
  const collection = useGraphCatalog(controlPort);
  const catalogErrorText = useCatalogError(collection.error);
  const { catalog, busy: catalogBusy, uncertain: catalogUncertain } = collection;
  const [sessionBusy, setSessionBusy] = useState(false);
  const deviceBufferAction = useDeviceBufferControl(controlPort, status, setSessionBusy, () =>
    setStatusRefresh((value) => value + 1),
  );
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const ownedSession = useOwnedSessionDocument(controlPort, collection.document);
  const [sessionDocument, setSessionDocument] = ownedSession;
  const { isCreatingGraph, setIsCreatingGraph, openNewGraph } = useGraphCreation(
    catalog?.activeId,
    paid || !catalog?.graphs.length,
  );
  const busy = catalogBusy || sessionBusy || workspaceBusy || catalogUncertain;
  const title = useCatalogTitle(collection, busy, t('studio.untitledGraph'));
  const controlsDisabled = busy || title.unsaved;
  const onSessionApplied = (document: RuntimeGraphDocument) => {
    setSessionDocument(document);
    setStatusRefresh((value) => value + 1);
  };
  const mutateCatalog = busy ? () => undefined : collection.mutate;
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
          notice={driverInstallNotice(driverSession, onInstallDriver)}
          headerActions={
            <WorkspaceFileMenu
              controlPort={controlPort}
              disabled={controlsDisabled}
              run={collection.workspaceFile}
            />
          }
          deviceDriverAction={driverRoutingActions(driverSession)}
          deviceBufferAction={bufferRoutingActions(deviceBufferAction, sessionBusy)}
          key={`${catalog?.activeId ?? 'engine'}:${collection.documentGeneration}`}
          graphId={catalog?.activeId}
          interactionDisabled={catalogBusy || catalogUncertain || sessionBusy}
          audioSession={status?.audioSession}
          processorTimings={status?.processorTimings}
          {...routingWorkspaceRuntimeProps(status, catalog)}
          sessionDocument={documentForWorkspace}
          controlPort={controlPort}
          onBusyChange={setWorkspaceBusy}
          onTitleChange={title.setDraft}
          title={catalog ? title.draft : undefined}
        />
      </section>

      <StudioStatusBar
        applicationInfo={applicationInfo}
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

function useGraphCreation(activeId: string | undefined, allowed: boolean) {
  const [isCreatingGraph, setIsCreatingGraph] = useState(false);
  useEffect(() => {
    if (activeId) setIsCreatingGraph(false);
  }, [activeId]);
  const openNewGraph = useCallback(() => {
    if (allowed) setIsCreatingGraph(true);
  }, [allowed]);
  useNewGraphShortcut(Boolean(activeId) && allowed, openNewGraph);
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
        <ProductHeading />
        <span className={styles.applicationActions}>
          <ThemePicker />
          {applicationActions}
        </span>
      </header>
      <GraphSidebar hidden={!sidebarOpen} {...sidebar} />
      <SidebarToggle
        label={t('studio.graphs')}
        opened={sidebarOpen}
        controls="graph-sidebar"
        onToggle={() => setSidebarOpen((open) => !open)}
      />
      {children}
    </main>
  );
}

function StudioStatusBar({
  applicationInfo,
  ...props
}: ComponentProps<typeof AudioPauseControl> & {
  applicationInfo?: ReactNode;
}) {
  return (
    <ApplicationStatusBar
      className={styles.statusBar}
      information={applicationInfo}
      controls={
        <>
          <AudioSessionNotices status={props.status} />
          <AudioPauseControl {...props} />
        </>
      }
    />
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
  const paid = usePaidFeatures();
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
            <ActionButton disabled={catalogBusy} onClick={() => void reloadCatalog()}>
              {t('studio.reloadGraphs')}
            </ActionButton>
          </div>
        ) : null}
        {catalog?.persistenceError ? <p role="alert">{t('studio.graphSaveFailed')}</p> : null}
        <div className={styles.sidebarFooter}>
          <IconActionButton
            aria-keyshortcuts={`${platformModifier()}+Shift+N`}
            label={t('studio.newGraph')}
            disabled={!catalog || (!paid && catalog.graphs.length >= 1)}
            aria-description={!paid ? t('licensing.limits.graphs') : undefined}
            onClick={onCreate}
          >
            <svg aria-hidden="true" viewBox="0 0 16 16">
              <path d="M8 3v10M3 8h10" />
            </svg>
          </IconActionButton>
        </div>
      </nav>
    </aside>
  );
}

function driverRoutingActions(session: DriverSession | undefined) {
  return {
    settings: session?.snapshot
      ? {
          ...session.snapshot.settings,
          canCreate: session.allowMultiDevice && session.snapshot.settings.canCreate,
          canRename: session.allowMultiDevice && session.snapshot.settings.canRename,
        }
      : undefined,
    enabled: session?.snapshot?.state === 'ready' && !session.busy,
    error: session?.error ? { endpointId: session.endpointId, message: session.error } : null,
    change: (
      endpointId: string,
      update: import('../features/routing-workspace/model').VirtualEndpointUpdate,
    ) => {
      void session?.changeDevice(endpointId, update);
    },
    create: async (spec: import('../core/control-port/types').VirtualEndpointSpec) => {
      const snapshot = await session?.mutate({ action: 'create', spec });
      return snapshot?.settings.devices.find((device) => device.spec.id === spec.id)?.spec ?? null;
    },
  };
}

function driverInstallNotice(session: DriverSession | undefined, onInstall?: () => void) {
  return (
    <DriverInstallNotice
      state={session?.snapshot?.state}
      onInstall={onInstall}
      className={styles.driverNotice}
    />
  );
}

function bufferRoutingActions(action: ReturnType<typeof useDeviceBufferControl>, busy: boolean) {
  return { ...action, enabled: action.enabled && !busy };
}
