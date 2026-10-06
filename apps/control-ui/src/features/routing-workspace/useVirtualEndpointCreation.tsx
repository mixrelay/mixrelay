import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AudioGraphModel, AudioGraphPosition } from '../../core/audio-graph/model';
import type { ControlPort, VirtualEndpointSettings, VirtualEndpointSpec } from '../../core/control-port/types';
import { NameDialog } from '../../core/ui/NameDialog';
import { addRoutingNode, type GraphPresentation } from './model';
import {
  resolveVirtualEndpointAction,
  type VirtualNodeSide,
  virtualEndpointNodeRequest,
} from './virtualEndpointMenu';

interface Creation {
  owner: ControlPort;
  side: VirtualNodeSide;
  position: AudioGraphPosition;
}

/** Identity is allocated only on confirmation; queue retries reuse the same graph snapshot. */
export function useVirtualEndpointCreation({
  owner,
  model,
  settings,
  presentation,
  enabled,
  updateModel,
  createDevice,
}: {
  owner: ControlPort;
  model: AudioGraphModel | null;
  settings?: VirtualEndpointSettings;
  presentation?: GraphPresentation;
  enabled: boolean;
  updateModel: (model: AudioGraphModel) => void;
  createDevice?: (spec: VirtualEndpointSpec) => Promise<VirtualEndpointSpec | null>;
}) {
  const [creation, setCreation] = useState<Creation | null>(null);
  const pendingIdentity = useRef<{ creation: Creation; id: string } | null>(null);
  const [creating, setCreating] = useState(false);
  const [failed, setFailed] = useState(false);
  const mounted = useRef(false);
  const current = useRef({ owner, model, creation, settings });
  current.current = { owner, model, creation, settings };
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    setCreation((pending) => (pending?.owner === owner ? pending : null));
    setCreating(false);
  }, [owner]);
  const { t } = useTranslation();
  const tryAdd = (itemId: string, position: AudioGraphPosition) => {
    if (!itemId.startsWith('virtual')) return false;
    if (!enabled || !model || !settings) return true;
    const action = resolveVirtualEndpointAction(itemId, settings);
    if (!action) return true;
    if (action.kind === 'create') {
      setCreation({ owner, side: action.side, position });
      setFailed(false);
    } else {
      const request = virtualEndpointNodeRequest(action.side, action.spec);
      if (!request) return true;
      updateModel(addRoutingNode(model, request, position, presentation, settings));
    }
    return true;
  };
  const create = async (displayName: string) => {
    if (
      !enabled ||
      !model ||
      !creation ||
      creation.owner !== owner ||
      !settings?.canCreate ||
      !settings.creationTemplate
    )
      return;
    if (creating) return;
    if (!createDevice) { setFailed(true); return; }
    const pending = creation;
    if (pendingIdentity.current?.creation !== creation)
      pendingIdentity.current = { creation, id: crypto.randomUUID() };
    const spec = { ...settings.creationTemplate, id: pendingIdentity.current.id, displayName };
    setCreating(true);
    setFailed(false);
    const actual = await createDevice(spec).catch(() => null);
    if (!mounted.current || current.current.owner !== owner || current.current.creation !== pending || !current.current.settings?.canCreate) return;
    if (!actual) { setFailed(true); setCreating(false); return; }
    const request = virtualEndpointNodeRequest(creation.side, actual);
    if (!request) { setFailed(true); setCreating(false); return; }
    const latest = current.current.model;
    if (latest) updateModel(addRoutingNode(latest, request, creation.position, presentation, settings));
    setCreating(false);
    setCreation(null);
  };
  return {
    tryAdd,
    dialog:
      creation?.owner === owner && settings?.canCreate ? (
        <>
        <NameDialog
          title={t('routing.nodes.newVirtualDevice')}
          initialName="MixRelay IO"
          nameLabel={t('routing.nodes.deviceName')}
          createLabel={t('common.create')}
          enabled={enabled && !creating}
          onCancel={() => { setCreation(null); setCreating(false); }}
          onCreate={(name) => void create(name)}
          error={failed ? t('driverManagement.changeFailed') : null}
        />
        </>
      ) : null,
  };
}
