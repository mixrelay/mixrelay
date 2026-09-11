import streamDeck from '@elgato/streamdeck';
import { exchange } from './client.mjs';
import { commandFor, presentation } from './controls.mjs';

const actions = new Map();
let snapshot;
let observedAt = 0;
let polling = false;
let writing = false;

async function render(entry) {
  const view = presentation(snapshot, entry.settings);
  if (entry.action.isDial())
    await entry.action.setFeedback({ title: view.title, value: view.value });
  else await entry.action.setTitle(`${view.title}\n${view.value}`);
}

async function publish() {
  await Promise.allSettled([...actions.values()].map(render));
  await streamDeck.ui.sendToPropertyInspector({ type: 'snapshot', snapshot: snapshot ?? null });
}

async function refresh() {
  if (polling || writing) return;
  polling = true;
  try {
    snapshot = await exchange({ type: 'snapshot' }, undefined, 4000);
    if (snapshot.protocolVersion !== 1) throw new Error('Unsupported MixRelay control protocol');
    observedAt = Date.now();
  } catch {
    snapshot = undefined;
  } finally {
    polling = false;
  }
  await publish();
}

async function control(event, operation, ticks) {
  const entry = actions.get(event.action.id);
  if (!entry) return;
  // Do not queue intent, rebase a stale click, or optimistically change key state.
  if (writing || polling || Date.now() - observedAt > 2500) return event.action.showAlert();
  writing = true;
  try {
    await exchange(commandFor(snapshot, entry.settings, operation, ticks));
  } catch (error) {
    streamDeck.logger.warn(String(error));
    await event.action.showAlert();
  } finally {
    snapshot = undefined;
    writing = false;
    await publish();
    await refresh();
  }
}

function update(event) {
  actions.set(event.action.id, { action: event.action, settings: event.payload.settings });
  void render(actions.get(event.action.id)).catch((error) => streamDeck.logger.warn(String(error)));
}

streamDeck.actions.onWillAppear(update);
streamDeck.settings.onDidReceiveSettings(update);
streamDeck.actions.onWillDisappear((event) => actions.delete(event.action.id));
streamDeck.actions.onKeyDown((event) => {
  void control(event).catch((error) => streamDeck.logger.warn(String(error)));
});
streamDeck.actions.onDialDown((event) => {
  void control(event, 'mute').catch((error) => streamDeck.logger.warn(String(error)));
});
streamDeck.actions.onDialRotate((event) => {
  void control(event, 'gain_up', event.payload.ticks).catch((error) =>
    streamDeck.logger.warn(String(error)),
  );
});
streamDeck.ui.onSendToPlugin((event) => {
  if (event.payload?.type === 'snapshot')
    void publish().catch((error) => streamDeck.logger.warn(String(error)));
});
await streamDeck.connect();
await refresh();
setInterval(() => {
  void refresh().catch((error) => streamDeck.logger.warn(String(error)));
}, 1000);
