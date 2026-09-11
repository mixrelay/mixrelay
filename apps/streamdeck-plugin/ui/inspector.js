import { operations } from '../src/capabilities.mjs';

let socket;
let context;
let settings = {};
let snapshot;
let dial = false;
const element = (id) => document.getElementById(id);
const labels = {
  power: 'On / Off',
  mute: 'Mute',
  gain_up: 'Volume +0.5 dB',
  gain_down: 'Volume −0.5 dB',
  record: 'Record / Resume',
  pause: 'Pause',
  stop: 'Stop',
};

function choices(id, items, selected) {
  const select = element(id);
  select.replaceChildren();
  const placeholder = new Option(
    selected && !items.some(([value]) => value === selected)
      ? 'Unavailable — choose another'
      : 'Choose…',
    '',
  );
  select.add(placeholder);
  for (const [value, label] of items) select.add(new Option(label, value));
  select.value = items.some(([value]) => value === selected) ? selected : '';
  select.disabled = !snapshot || items.length === 0;
}

function render() {
  element('status').textContent = snapshot
    ? 'Connected to MixRelay'
    : 'Open MixRelay to select a node.';
  const graphs = snapshot?.graphs ?? [];
  choices(
    'graph',
    graphs.map((graph) => [graph.id, graph.name]),
    settings.graphId,
  );
  const nodes = graphs.find((graph) => graph.id === settings.graphId)?.nodes ?? [];
  choices(
    'node',
    nodes
      .filter((node) => !dial || Number.isFinite(node.gainDb))
      .map((node) => [node.nodeId, node.label || node.nodeId]),
    settings.nodeId,
  );
  const node = nodes.find((node) => node.nodeId === settings.nodeId);
  const available = operations(node);
  choices(
    'operation',
    available.map((value) => [value, labels[value]]),
    settings.operation,
  );
  element('operation-label').hidden = dial;
  element('folder-label').hidden =
    !node?.capabilities.recorder || settings.operation !== 'record' || dial;
  if (document.activeElement !== element('folder'))
    element('folder').value = settings.recordingFolder ?? '';
  element('hint').textContent = dial
    ? 'Rotate: volume · Press: mute. OFF nodes ignore these controls.'
    : 'Commands target this node, even when another graph is selected in MixRelay.';
}

function save() {
  socket.send(JSON.stringify({ event: 'setSettings', context, payload: settings }));
  render();
}
element('graph').onchange = () => {
  settings = {
    ...settings,
    graphId: element('graph').value,
    nodeId: '',
    operation: dial ? 'gain_up' : '',
  };
  save();
};
element('node').onchange = () => {
  settings = { ...settings, nodeId: element('node').value, operation: dial ? 'gain_up' : '' };
  save();
};
element('operation').onchange = () => {
  settings.operation = element('operation').value;
  save();
};
element('folder').onchange = () => {
  settings.recordingFolder = element('folder').value.trim();
  save();
};

window.connectElgatoStreamDeckSocket = (port, uuid, registerEvent, _info, actionInfo) => {
  context = uuid;
  const action = JSON.parse(actionInfo);
  settings = action.payload.settings;
  dial = action.payload.controller === 'Encoder';
  socket = new WebSocket(`ws://127.0.0.1:${port}`);
  socket.onopen = () => {
    socket.send(JSON.stringify({ event: registerEvent, uuid }));
    socket.send(
      JSON.stringify({
        event: 'sendToPlugin',
        action: action.action,
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
