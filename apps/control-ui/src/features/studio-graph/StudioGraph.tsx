import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { ActionButton } from '../../core/ui/ActionButton';
import { AudioNodeCard } from '../../core/ui/AudioNodeCard';
import { LevelControl } from '../../core/ui/LevelControl';
import { Meter } from '../../core/ui/Meter';
import { StatusBadge, type StatusTone } from '../../core/ui/StatusBadge';
import { ToggleField } from '../../core/ui/ToggleField';
import {
  broadcastConnections,
  broadcastGraph,
  type CapabilityState,
  findCapability,
  findGraphNode,
  type GraphNode,
  type NodeGroup,
  nodeCatalog,
} from './model';

const paletteNodeIds = [
  'application',
  'inputDevice',
  'mixer',
  'compressor',
  'parametricEq',
  'virtualDevice',
];

const stateTone: Record<CapabilityState, StatusTone> = {
  foundation: 'positive',
  planned: 'warning',
  provider: 'neutral',
};

export function StudioGraph() {
  const { t } = useTranslation();
  const [selectedNodeId, setSelectedNodeId] = useState('compressor');
  const [bypassedNodeIds, setBypassedNodeIds] = useState<ReadonlySet<string>>(new Set());
  const [outputLevel, setOutputLevel] = useState(-3);
  const selectedNode = useMemo(() => findGraphNode(selectedNodeId), [selectedNodeId]);

  const setBypassed = (bypassed: boolean) => {
    setBypassedNodeIds((current) => {
      const next = new Set(current);
      if (bypassed) next.add(selectedNode.id);
      else next.delete(selectedNode.id);
      return next;
    });
  };

  return (
    <section className="graph-studio" aria-label={t('studioGraph.editorLabel')}>
      <StudioToolbar />
      <div className="graph-studio__body">
        <NodePalette />
        <div className="graph-workspace">
          <section className="graph-canvas" aria-label={t('studioGraph.canvasLabel')}>
            <div className="graph-canvas__grid" aria-hidden="true" />
            <svg className="graph-connections" viewBox="0 0 1360 620" aria-hidden="true">
              {broadcastConnections.map((connection) => (
                <path
                  className={`graph-connection graph-connection--${connection.tone}`}
                  d={connection.path}
                  key={connection.id}
                />
              ))}
            </svg>
            {broadcastGraph.map((node) => (
              <GraphNodeCard
                bypassed={bypassedNodeIds.has(node.id)}
                key={node.id}
                node={node}
                onSelect={setSelectedNodeId}
                selected={node.id === selectedNode.id}
              />
            ))}
            <div className="canvas-note canvas-note--source">{t('studioGraph.flow.capture')}</div>
            <div className="canvas-note canvas-note--shape">{t('studioGraph.flow.shape')}</div>
            <div className="canvas-note canvas-note--deliver">{t('studioGraph.flow.deliver')}</div>
          </section>
          <footer className="graph-statusbar">
            <span>{t('studioGraph.status.nodes', { count: broadcastGraph.length })}</span>
            <span>{t('studioGraph.status.channels')}</span>
            <span className="graph-statusbar__healthy">{t('studioGraph.status.healthy')}</span>
          </footer>
        </div>
        <NodeInspector
          bypassed={bypassedNodeIds.has(selectedNode.id)}
          node={selectedNode}
          onBypassChange={setBypassed}
          onOutputLevelChange={setOutputLevel}
          outputLevel={outputLevel}
        />
      </div>
    </section>
  );
}

export function NodeInventory() {
  const { t } = useTranslation();
  const groups: NodeGroup[] = ['source', 'processor', 'analysis', 'output'];

  return (
    <section className="node-inventory">
      <header className="node-inventory__header">
        <div>
          <p className="eyebrow">{t('studioGraph.inventory.eyebrow')}</p>
          <h1>{t('studioGraph.inventory.title')}</h1>
          <p>{t('studioGraph.inventory.description')}</p>
        </div>
        <InventoryLegend />
      </header>
      {groups.map((group) => (
        <section className="node-inventory__group" key={group}>
          <h2>{t(`studioGraph.groups.${group}`)}</h2>
          <span>{nodeCatalog.filter((node) => node.group === group).length}</span>
          <div className="node-inventory__grid">
            {nodeCatalog
              .filter((node) => node.group === group)
              .map((node) => (
                <article className="capability-card" key={node.id}>
                  <span className="capability-card__icon" aria-hidden="true">
                    {node.icon}
                  </span>
                  <div>
                    <strong>{t(node.nameKey)}</strong>
                    <p>{t(node.descriptionKey)}</p>
                  </div>
                  <StatusBadge tone={stateTone[node.state]}>
                    {t(`studioGraph.states.${node.state}`)}
                  </StatusBadge>
                </article>
              ))}
          </div>
        </section>
      ))}
    </section>
  );
}

function StudioToolbar() {
  const { t } = useTranslation();
  return (
    <header className="graph-toolbar">
      <div className="graph-toolbar__brand">
        <span className="graph-toolbar__mark">TN</span>
        <div>
          <strong>{t('studioGraph.sessionName')}</strong>
          <small>{t('studioGraph.saved')}</small>
        </div>
      </div>
      <div className="graph-toolbar__transport">
        <span className="live-dot" aria-hidden="true" />
        <span>{t('studioGraph.running')}</span>
        <code>48 kHz</code>
        <code>128 spl</code>
      </div>
      <div className="graph-toolbar__actions">
        <ActionButton color="gray" size="xs">
          {t('studioGraph.remote')}
        </ActionButton>
        <ActionButton size="xs">{t('studioGraph.addNode')}</ActionButton>
      </div>
    </header>
  );
}

function NodePalette() {
  const { t } = useTranslation();
  return (
    <aside className="node-palette">
      <label className="node-palette__search">
        <span className="sr-only">{t('studioGraph.search')}</span>
        <span aria-hidden="true">⌕</span>
        <input placeholder={t('studioGraph.search')} readOnly />
      </label>
      <p className="node-palette__heading">{t('studioGraph.library')}</p>
      {paletteNodeIds.map((id) => {
        const node = findCapability(id);
        return (
          <button className="palette-item" key={node.id} type="button">
            <span aria-hidden="true">{node.icon}</span>
            <span>
              <strong>{t(node.nameKey)}</strong>
              <small>{t(`studioGraph.groups.${node.group}`)}</small>
            </span>
            <span aria-hidden="true">+</span>
          </button>
        );
      })}
      <p className="node-palette__hint">{t('studioGraph.dragHint')}</p>
    </aside>
  );
}

function GraphNodeCard({
  bypassed,
  node,
  onSelect,
  selected,
}: {
  bypassed: boolean;
  node: GraphNode;
  onSelect: (id: string) => void;
  selected: boolean;
}) {
  const { t } = useTranslation();
  const capability = findCapability(node.capabilityId);
  return (
    <div className={`graph-node-position graph-node-position--${node.placement}`}>
      <AudioNodeCard
        bypassed={bypassed}
        category={t(`studioGraph.groups.${capability.group}`)}
        detail={t(`studioGraph.details.${node.detailKey}`)}
        icon={capability.icon}
        inputCount={node.inputCount}
        onSelect={() => onSelect(node.id)}
        outputCount={node.outputCount}
        selected={selected}
        status={node.id === 'obs' ? 'OBS' : undefined}
        statusTone="positive"
        title={t(capability.nameKey)}
      />
      <div className="node-mini-meter" aria-hidden="true">
        <span style={{ width: node.id === 'microphone' ? '58%' : '76%' }} />
      </div>
    </div>
  );
}

function NodeInspector({
  bypassed,
  node,
  onBypassChange,
  onOutputLevelChange,
  outputLevel,
}: {
  bypassed: boolean;
  node: GraphNode;
  onBypassChange: (bypassed: boolean) => void;
  onOutputLevelChange: (level: number) => void;
  outputLevel: number;
}) {
  const { t } = useTranslation();
  const capability = findCapability(node.capabilityId);
  return (
    <aside className="node-inspector">
      <header>
        <span className="node-inspector__icon" aria-hidden="true">
          {capability.icon}
        </span>
        <div>
          <small>{t('studioGraph.inspector')}</small>
          <h2>{t(capability.nameKey)}</h2>
        </div>
      </header>
      <div className="inspector-section">
        <ToggleField
          checked={bypassed}
          label={t('studioGraph.controls.bypass')}
          onChange={onBypassChange}
        />
      </div>
      <div className="inspector-section">
        <LevelControl
          label={t('studioGraph.controls.output')}
          max={12}
          min={-24}
          onChange={onOutputLevelChange}
          suffix="dB"
          value={outputLevel}
        />
        <LevelControl
          label={t('studioGraph.controls.mix')}
          max={100}
          min={0}
          onChange={() => undefined}
          suffix="%"
          value={100}
        />
      </div>
      <div className="inspector-section inspector-routing">
        <span>{t('studioGraph.controls.routing')}</span>
        <code>2 in → 2 out</code>
      </div>
      <div className="inspector-meter">
        <div>
          <Meter label="L" level={78} />
          <Meter label="R" level={71} />
        </div>
        <code>-9.2 dBFS</code>
      </div>
    </aside>
  );
}

function InventoryLegend() {
  const { t } = useTranslation();
  return (
    <div className="inventory-legend">
      {(['foundation', 'planned', 'provider'] as const).map((state) => (
        <StatusBadge key={state} tone={stateTone[state]}>
          {t(`studioGraph.states.${state}`)}
        </StatusBadge>
      ))}
    </div>
  );
}
