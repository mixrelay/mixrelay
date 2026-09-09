import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AudioGraphModel, AudioGraphPosition } from '../../core/audio-graph/model';
import type { ControlPort, VirtualEndpointSettings } from '../../core/control-port/types';
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
}: {
  owner: ControlPort;
  model: AudioGraphModel | null;
  settings?: VirtualEndpointSettings;
  presentation?: GraphPresentation;
  enabled: boolean;
  updateModel: (model: AudioGraphModel) => void;
}) {
  const [creation, setCreation] = useState<Creation | null>(null);
  useEffect(() => {
    setCreation((pending) => (pending?.owner === owner ? pending : null));
  }, [owner]);
  const { t } = useTranslation();
  const tryAdd = (itemId: string, position: AudioGraphPosition) => {
    if (!itemId.startsWith('virtual')) return false;
    if (!enabled || !model || !settings) return true;
    const action = resolveVirtualEndpointAction(itemId, settings);
    if (!action) return true;
    if (action.kind === 'create') {
      setCreation({ owner, side: action.side, position });
    } else {
      const request = virtualEndpointNodeRequest(action.side, action.spec);
      if (!request) return true;
      updateModel(addRoutingNode(model, request, position, presentation, settings));
    }
    return true;
  };
  const create = (displayName: string) => {
    if (
      !enabled ||
      !model ||
      !creation ||
      creation.owner !== owner ||
      !settings?.canCreate ||
      !settings.creationTemplate
    )
      return;
    const spec = { ...settings.creationTemplate, id: crypto.randomUUID(), displayName };
    const request = virtualEndpointNodeRequest(creation.side, spec);
    if (!request) return;
    updateModel(addRoutingNode(model, request, creation.position, presentation, settings));
    setCreation(null);
  };
  return {
    tryAdd,
    dialog:
      creation?.owner === owner ? (
        <NameDialog
          title={t('routing.nodes.newVirtualDevice')}
          initialName="ToneNari IO"
          nameLabel={t('routing.nodes.deviceName')}
          createLabel={t('common.create')}
          enabled={enabled}
          onCancel={() => setCreation(null)}
          onCreate={create}
        />
      ) : null,
  };
}
