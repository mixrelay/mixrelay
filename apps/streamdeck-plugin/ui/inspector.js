import { actionIds, actionSettings, supportsAction, volumeSteps } from '../src/action-settings.mjs';
import { translate } from '../src/localization.mjs';
import { recordingSettingsSummary } from '../src/recording-settings.mjs';

let socket;
let context;
let actionId;
let language = 'en';
let settings = {};
let snapshot;
let dial = false;
const element = (id) => document.getElementById(id);
const t = (key) => translate(language, key);

function choices(id, items, selected) {
  const select = element(id);
  select.replaceChildren();
  const allowsEmpty = id === 'graph' || id === 'node';
  if (allowsEmpty)
    select.add(
      new Option(
        t(
          selected && !items.some(([value]) => value === selected)
            ? 'Unavailable — choose another'
            : 'Choose…',
        ),
        '',
      ),
    );
  for (const [value, label] of items) select.add(new Option(label, value));
  select.value = items.some(([value]) => value === selected)
    ? selected
    : allowsEmpty
      ? ''
      : (items[0]?.[0] ?? '');
  select.disabled = !snapshot || items.length === 0;
}

function render() {
  for (const label of document.querySelectorAll('[data-i18n]'))
    label.textContent = t(label.dataset.i18n);
  element('status').textContent = t(
    snapshot ? 'Connected to MixRelay' : 'Open MixRelay to select a node.',
  );
  element('status').hidden = !!snapshot;
  const graphs = snapshot?.graphs ?? [];
  choices(
    'graph',
    graphs.map((graph) => [graph.id, graph.name]),
    settings.graphId,
  );
  const nodes = (graphs.find((graph) => graph.id === settings.graphId)?.nodes ?? []).filter(
    (node) => supportsAction(actionId, node),
  );
  choices(
    'node',
    nodes.map((node) => [node.nodeId, node.label || node.nodeId]),
    settings.nodeId,
  );
  const volume = actionId === actionIds.volume;
  const recording = actionId === actionIds.recording;
  const selected = actionSettings(actionId, settings, dial);
  choices(
    'operation',
    [
      ['record', t('Record')],
      ['pause', t('Pause')],
      ['stop', t('Stop')],
    ],
    selected.operation,
  );
  choices(
    'direction',
    [
      ['up', t('Increase')],
      ['down', t('Decrease')],
    ],
    settings.direction ?? 'up',
  );
  choices(
    'step',
    volumeSteps.map((step) => [String(step), `${step} dB`]),
    String(settings.stepDb ?? 0.5),
  );
  element('operation-label').hidden = !recording;
  element('direction-label').hidden = !volume || dial;
  element('step-label').hidden = !volume;
  element('recording-settings').hidden = !recording;
  const summary = recordingSettingsSummary(
    nodes.find((node) => node.nodeId === settings.nodeId),
    language,
  );
  element('recording-folder').textContent = summary.folder;
  element('recording-format').textContent = summary.format;
  const hint =
    nodes.length === 0 && settings.graphId
      ? 'No supported nodes in this graph.'
      : volume && dial
        ? 'Rotate: volume · Press: mute.'
        : '';
  element('hint').textContent = hint ? t(hint) : '';
  element('hint').hidden = !hint;
}

function save() {
  socket.send(JSON.stringify({ event: 'setSettings', context, payload: settings }));
  render();
}
element('graph').onchange = () => {
  settings = { ...settings, graphId: element('graph').value, nodeId: '' };
  save();
};
for (const [id, key] of [
  ['node', 'nodeId'],
  ['operation', 'operation'],
  ['direction', 'direction'],
]) {
  element(id).onchange = () => {
    settings[key] = element(id).value.trim();
    save();
  };
}
element('step').onchange = () => {
  settings.stepDb = Number(element('step').value);
  save();
};

window.connectElgatoStreamDeckSocket = (port, uuid, registerEvent, info, actionInfo) => {
  context = uuid;
  language = JSON.parse(info).application.language;
  document.documentElement.lang = language;
  const action = JSON.parse(actionInfo);
  actionId = action.action;
  settings = action.payload.settings;
  dial = action.payload.controller === 'Encoder';
  socket = new WebSocket(`ws://127.0.0.1:${port}`);
  socket.onopen = () => {
    socket.send(JSON.stringify({ event: registerEvent, uuid }));
    socket.send(
      JSON.stringify({
        event: 'sendToPlugin',
        action: actionId,
        context,
        payload: { type: 'snapshot' },
      }),
    );
  };
  socket.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.event === 'sendToPropertyInspector' && message.payload.type === 'snapshot') {
      snapshot = message.payload.snapshot;
      render();
    }
  };
  socket.onclose = () => {
    snapshot = undefined;
    render();
  };
  render();
};
