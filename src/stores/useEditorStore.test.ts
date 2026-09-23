/**
 * The editor's store, against the rules the edit model is supposed to keep:
 * that the way back only holds steps that changed something, and holds a drag
 * or a slider's sweep as the one step it felt like; that a clip's history and
 * its drawn tracks belong to that clip alone, and survive going to look at
 * another; and that a file the app cannot measure is refused rather than
 * opened as a fraction of a second.
 *
 * The store is the only place those rules can be seen -- the helpers under it
 * are pure and already honest about refusing -- so the backend is replaced by
 * the smallest thing that answers.
 */

import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

import { en } from '@/i18n/en';
import type { MediaProbe, TimelineState } from '@/types';

const backend = vi.hoisted(() => ({
  probe: null as unknown as MediaProbe,
  /** What the probe rejects with instead of answering, when set. */
  failure: null as unknown,
}));

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock('@/services/ipc', () => ({
  probeMedia: async () => {
    if (backend.failure != null) throw backend.failure;
    return backend.probe;
  },
  allowMediaPreview: async () => {},
  mediaKeyframes: async () => [],
  requestTimeline: async () => {},
  startExport: async () => {},
  cancelExport: async () => {},
  cancelRangeFetch: async () => {},
  toAppError: (value: unknown) => ({
    code: 'unknown',
    title: 'Something went wrong',
    message: String(value),
    technical: null,
    retryable: true,
  }),
}));

const { selectCuts, selectHistory, selectOptions, useEditorStore } = await import(
  '@/stores/useEditorStore'
);

function probeOf(patch: Partial<MediaProbe> = {}): MediaProbe {
  return {
    path: 'C:/tmp/a.mp4',
    fileName: 'a.mp4',
    container: 'mp4',
    sizeBytes: 1_000_000,
    durationSec: 20,
    width: 1920,
    height: 1080,
    fps: 30,
    videoDurationSec: 20,
    pixelAspect: null,
    videoCodec: 'h264',
    audioCodec: 'aac',
    audioBitrateKbps: 192,
    hasVideo: true,
    hasAudio: true,
    ...patch,
  };
}

async function openClip(path: string, patch: Partial<MediaProbe> = {}): Promise<string> {
  backend.probe = probeOf({ path, fileName: path, ...patch });
  const id = await useEditorStore.getState().open(path, false);
  if (!id) throw new Error(`the store refused to open ${path}`);
  return id;
}

function drawn(path: string): TimelineState {
  return {
    path,
    token: 1,
    working: false,
    waveform: { startSec: 0, lengthSec: 20, peaks: [0.5] } as never,
    filmstrip: { startSec: 0, lengthSec: 20, cells: [] } as never,
    error: null,
  };
}

/** Steps back, and forward, on the clip being edited. */
const back = () => selectHistory(useEditorStore.getState()).past.length;
const forward = () => selectHistory(useEditorStore.getState()).future.length;

/** The clip being edited, as the numbers an export would be made from. */
function edit() {
  const state = useEditorStore.getState();
  return { cuts: selectCuts(state), options: selectOptions(state) };
}

/**
 * Let the clock move on. A run of changes to one setting is told apart from
 * two separate changes by time alone, so the tests keep the clock in hand.
 */
function pause(ms: number) {
  vi.setSystemTime(Date.now() + ms);
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(new Date('2026-01-01T00:00:00Z'));
  useEditorStore.getState().closeAll();
  backend.probe = probeOf();
  backend.failure = null;
});

afterEach(() => {
  vi.useRealTimers();
});

describe('the way back', () => {
  test('a split the pieces cannot take is not a step', async () => {
    await openClip('C:/tmp/a.mp4');
    const store = useEditorStore.getState();

    store.split(0);

    expect(selectCuts(useEditorStore.getState())).toHaveLength(1);
    expect(back()).toBe(0);

    store.split(10);

    expect(selectCuts(useEditorStore.getState())).toHaveLength(2);
    expect(back()).toBe(1);
  });

  test('a delete of the last piece is not a step', async () => {
    await openClip('C:/tmp/a.mp4');
    const only = selectCuts(useEditorStore.getState())[0]!;

    useEditorStore.getState().remove(only.id);

    expect(selectCuts(useEditorStore.getState())).toHaveLength(1);
    expect(back()).toBe(0);
  });

  test('closing another clip leaves the open one and its history alone', async () => {
    const first = await openClip('C:/tmp/a.mp4');
    const second = await openClip('C:/tmp/b.mp4');
    useEditorStore.getState().split(10);
    expect(back()).toBe(1);

    useEditorStore.getState().closeClip(first);

    expect(useEditorStore.getState().activeId).toBe(second);
    expect(selectCuts(useEditorStore.getState())).toHaveLength(2);
    expect(back()).toBe(1);
  });

  test('closing the clip being edited takes its history with it', async () => {
    await openClip('C:/tmp/a.mp4');
    const second = await openClip('C:/tmp/b.mp4');
    useEditorStore.getState().split(10);

    useEditorStore.getState().closeClip(second);

    expect(back()).toBe(0);
    expect(useEditorStore.getState().history[second]).toBeUndefined();
  });

  test('a patch that changes nothing is not a step, and keeps the way forward', async () => {
    await openClip('C:/tmp/a.mp4');
    const store = useEditorStore.getState();
    store.split(10);
    store.undo();
    expect(forward()).toBe(1);

    // What a picker sends when its own value is chosen again.
    store.setOptions({ mute: false });
    store.setOptions({ container: 'mp4', mode: 'lossless' });

    expect(back()).toBe(0);
    expect(forward()).toBe(1);
  });

  test('an edge put back where it already was is not a step', async () => {
    await openClip('C:/tmp/a.mp4');
    const piece = selectCuts(useEditorStore.getState())[0]!;

    useEditorStore.getState().drag(piece.id, 'start', 0);

    expect(back()).toBe(0);
  });
});

describe('a drag is one step', () => {
  test('however many moves it took', async () => {
    await openClip('C:/tmp/a.mp4');
    const piece = selectCuts(useEditorStore.getState())[0]!;

    useEditorStore.getState().beginGesture();
    for (let tenth = 1; tenth <= 30; tenth += 1) {
      useEditorStore.getState().drag(piece.id, 'start', tenth / 10);
    }
    useEditorStore.getState().endGesture();

    expect(selectCuts(useEditorStore.getState())[0]!.startSec).toBe(3);
    expect(back()).toBe(1);

    useEditorStore.getState().undo();

    expect(selectCuts(useEditorStore.getState())[0]!.startSec).toBe(0);
    expect(back()).toBe(0);
    expect(forward()).toBe(1);
  });

  test('and none at all when it was let go of where it started', async () => {
    await openClip('C:/tmp/a.mp4');
    const store = useEditorStore.getState();
    store.split(10);
    store.undo();
    const piece = selectCuts(useEditorStore.getState())[0]!;

    store.beginGesture();
    store.drag(piece.id, 'end', 12);
    store.drag(piece.id, 'end', 20);
    store.endGesture();

    // A press that never moved.
    store.beginGesture();
    store.endGesture();

    expect(back()).toBe(0);
    // Nothing changed, so there is nothing for the way forward to disagree with.
    expect(forward()).toBe(1);
  });

  test('an edge moved from the keyboard is a step each time', async () => {
    await openClip('C:/tmp/a.mp4');
    const piece = selectCuts(useEditorStore.getState())[0]!;

    useEditorStore.getState().drag(piece.id, 'start', 2);
    useEditorStore.getState().drag(piece.id, 'start', 4);

    expect(back()).toBe(2);
  });

  test('an undo that arrives mid-drag closes the drag first', async () => {
    await openClip('C:/tmp/a.mp4');
    const piece = selectCuts(useEditorStore.getState())[0]!;

    useEditorStore.getState().beginGesture();
    useEditorStore.getState().drag(piece.id, 'start', 3);
    useEditorStore.getState().undo();

    expect(selectCuts(useEditorStore.getState())[0]!.startSec).toBe(0);
    expect(forward()).toBe(1);

    // The release that finally arrives has nothing left to record.
    useEditorStore.getState().endGesture();
    expect(back()).toBe(0);
    expect(forward()).toBe(1);
  });

  test('a drag whose release never came cannot swallow the next change', async () => {
    await openClip('C:/tmp/a.mp4');
    const piece = selectCuts(useEditorStore.getState())[0]!;

    useEditorStore.getState().beginGesture();
    useEditorStore.getState().drag(piece.id, 'start', 3);
    useEditorStore.getState().split(10);

    expect(back()).toBe(2);
  });

  test('and a pointer still down after an undo drags on as one more step, not one per move', async () => {
    await openClip('C:/tmp/a.mp4');
    const piece = selectCuts(useEditorStore.getState())[0]!;
    const store = useEditorStore.getState();

    store.beginGesture();
    store.drag(piece.id, 'start', 3, true);
    store.undo();
    for (let tenth = 31; tenth <= 50; tenth += 1) store.drag(piece.id, 'start', tenth / 10, true);
    store.endGesture();

    expect(selectCuts(useEditorStore.getState())[0]!.startSec).toBe(5);
    expect(back()).toBe(1);
    expect(forward()).toBe(0);

    store.undo();

    expect(selectCuts(useEditorStore.getState())[0]!.startSec).toBe(0);
  });
});

describe('a run of changes to one setting is one step', () => {
  test('a slider swept across many values', async () => {
    await openClip('C:/tmp/a.mp4');

    for (const volume of [1.05, 1.1, 1.25, 1.4, 1.6]) {
      pause(30);
      useEditorStore.getState().setOptions({ volume });
    }

    expect(edit().options!.volume).toBe(1.6);
    expect(back()).toBe(1);

    useEditorStore.getState().undo();

    expect(edit().options!.volume).toBe(1);
  });

  test('a number typed a keystroke at a time', async () => {
    await openClip('C:/tmp/a.mp4');

    for (const videoBitrateKbps of [8, 80, 800, 8000]) {
      pause(250);
      useEditorStore.getState().setOptions({ videoBitrateKbps });
    }

    expect(back()).toBe(1);
  });

  test('but a pause long enough starts another', async () => {
    await openClip('C:/tmp/a.mp4');

    useEditorStore.getState().setOptions({ audioBitrateKbps: 160 });
    pause(1500);
    useEditorStore.getState().setOptions({ audioBitrateKbps: 128 });

    expect(back()).toBe(2);
  });

  test('and so does a different setting, or an undo in between', async () => {
    await openClip('C:/tmp/a.mp4');
    const store = useEditorStore.getState();

    store.setOptions({ audioBitrateKbps: 160 });
    store.setOptions({ mute: true });
    expect(back()).toBe(2);

    store.undo();
    store.setOptions({ mute: true });
    expect(back()).toBe(2);
    expect(forward()).toBe(0);
  });

  test('a sweep that comes back to where it began leaves nothing behind', async () => {
    await openClip('C:/tmp/a.mp4');

    useEditorStore.getState().setOptions({ audioBitrateKbps: 160 });
    pause(40);
    useEditorStore.getState().setOptions({ audioBitrateKbps: 192 });

    expect(back()).toBe(0);
  });
});

describe('undo and redo', () => {
  test('walk the same steps in both directions, exactly', async () => {
    await openClip('C:/tmp/a.mp4');
    const seen = [edit()];
    const store = useEditorStore.getState();

    store.split(10);
    seen.push(edit());
    pause(1000);
    store.setOptions({ volume: 1.5 });
    seen.push(edit());
    const first = selectCuts(useEditorStore.getState())[0]!;
    store.beginGesture();
    store.drag(first.id, 'start', 2);
    store.drag(first.id, 'start', 4);
    store.endGesture();
    seen.push(edit());
    store.remove(selectCuts(useEditorStore.getState())[1]!.id);
    seen.push(edit());

    for (let index = seen.length - 2; index >= 0; index -= 1) {
      store.undo();
      expect(edit()).toEqual(seen[index]);
    }
    expect(back()).toBe(0);

    for (let index = 1; index < seen.length; index += 1) {
      store.redo();
      expect(edit()).toEqual(seen[index]);
    }
    expect(forward()).toBe(0);
  });

  test('a new change ends the way forward', async () => {
    await openClip('C:/tmp/a.mp4');
    const store = useEditorStore.getState();
    store.split(5);
    store.split(10);
    store.undo();
    expect(forward()).toBe(1);

    store.split(15);

    expect(forward()).toBe(0);
    const before = edit();
    store.redo();
    expect(edit()).toEqual(before);
  });

  test('clear what the last export said, but leave one that is running', async () => {
    await openClip('C:/tmp/a.mp4');
    const store = useEditorStore.getState();
    store.split(10);

    useEditorStore.setState({
      job: { status: 'completed', percent: null, outputPath: 'C:/tmp/out.mp4', error: null },
    });
    store.undo();
    expect(useEditorStore.getState().job.status).toBe('idle');

    const running = { status: 'running' as const, percent: 40, outputPath: null, error: null };
    useEditorStore.setState({ job: running });
    store.redo();
    expect(useEditorStore.getState().job).toBe(running);
  });
});

describe('each clip keeps its own way back', () => {
  test('through going to look at another and coming back', async () => {
    const first = await openClip('C:/tmp/a.mp4');
    useEditorStore.getState().split(5);
    useEditorStore.getState().split(10);
    expect(back()).toBe(2);

    // A clip fetched from a link opens as a new one, beside the first.
    const second = await openClip('C:/tmp/b.mp4');
    expect(back()).toBe(0);
    useEditorStore.getState().split(8);
    expect(back()).toBe(1);

    useEditorStore.getState().activate(first);
    expect(back()).toBe(2);
    useEditorStore.getState().undo();
    expect(selectCuts(useEditorStore.getState())).toHaveLength(2);

    useEditorStore.getState().activate(second);
    expect(back()).toBe(1);
    expect(selectCuts(useEditorStore.getState())).toHaveLength(2);

    useEditorStore.getState().activate(first);
    expect(back()).toBe(1);
    expect(forward()).toBe(1);
  });

  test('and an undo on one never reaches the other', async () => {
    const first = await openClip('C:/tmp/a.mp4');
    useEditorStore.getState().split(5);
    await openClip('C:/tmp/b.mp4');

    useEditorStore.getState().undo();

    expect(selectCuts(useEditorStore.getState())).toHaveLength(1);
    useEditorStore.getState().activate(first);
    expect(selectCuts(useEditorStore.getState())).toHaveLength(2);
  });

  test('a drag still held when the clip changes stays with the clip it began on', async () => {
    const first = await openClip('C:/tmp/a.mp4');
    const piece = selectCuts(useEditorStore.getState())[0]!;
    const second = await openClip('C:/tmp/b.mp4');
    useEditorStore.getState().activate(first);

    useEditorStore.getState().beginGesture();
    useEditorStore.getState().drag(piece.id, 'start', 3);
    useEditorStore.getState().activate(second);

    expect(back()).toBe(0);
    useEditorStore.getState().activate(first);
    expect(back()).toBe(1);
  });

  test('closing them all takes every way back with it', async () => {
    await openClip('C:/tmp/a.mp4');
    useEditorStore.getState().split(5);
    await openClip('C:/tmp/b.mp4');
    useEditorStore.getState().split(5);

    useEditorStore.getState().closeAll();

    expect(useEditorStore.getState().history).toEqual({});
  });
});

describe('what is drawn belongs to one clip', () => {
  test('switching clips drops the tracks drawn for the last one', async () => {
    const first = await openClip('C:/tmp/a.mp4');
    const second = await openClip('C:/tmp/b.mp4');

    useEditorStore.getState().activate(first);
    useEditorStore.getState().applyTimeline(drawn('C:/tmp/a.mp4'));
    expect(useEditorStore.getState().timeline.waveform).not.toBeNull();

    useEditorStore.getState().activate(second);

    expect(useEditorStore.getState().timeline.path).toBeNull();
    expect(useEditorStore.getState().timeline.waveform).toBeNull();
    expect(useEditorStore.getState().timeline.filmstrip).toBeNull();
  });

  test('closing the clip being edited drops them too', async () => {
    const first = await openClip('C:/tmp/a.mp4');
    await openClip('C:/tmp/b.mp4');
    useEditorStore.getState().activate(first);
    useEditorStore.getState().applyTimeline(drawn('C:/tmp/a.mp4'));

    useEditorStore.getState().closeClip(first);

    expect(useEditorStore.getState().timeline.waveform).toBeNull();
  });

  test('a clip opened after one was drawn starts empty', async () => {
    await openClip('C:/tmp/a.mp4');
    useEditorStore.getState().applyTimeline(drawn('C:/tmp/a.mp4'));

    await openClip('C:/tmp/b.mp4');

    expect(useEditorStore.getState().timeline.waveform).toBeNull();
  });
});

describe('opening', () => {
  test('a file whose length cannot be read is refused, and says so', async () => {
    backend.probe = probeOf({ path: 'C:/tmp/nodur.mkv', durationSec: null });

    const id = await useEditorStore.getState().open('C:/tmp/nodur.mkv', false);

    expect(id).toBeNull();
    expect(useEditorStore.getState().pool).toHaveLength(0);
    expect(useEditorStore.getState().openError?.message).toBe(en['editor.unknownLength']);
  });

  test('a file with no picture is refused, and says so', async () => {
    backend.probe = probeOf({
      path: 'C:/tmp/podcast.mp3',
      container: 'mp3',
      width: null,
      height: null,
      fps: null,
      videoDurationSec: null,
      videoCodec: null,
      hasVideo: false,
    });

    const id = await useEditorStore.getState().open('C:/tmp/podcast.mp3', false);

    expect(id).toBeNull();
    expect(useEditorStore.getState().pool).toHaveLength(0);
    expect(useEditorStore.getState().opening).toBe(false);
    expect(useEditorStore.getState().openError?.message).toBe(en['editor.noVideo']);
  });

  test('a file the backend cannot read is said in the editor’s words, not a download’s', async () => {
    backend.failure = 'ffprobe exited with status 1';

    const id = await useEditorStore.getState().open('C:/tmp/broken.mp4', false);

    expect(id).toBeNull();
    expect(useEditorStore.getState().opening).toBe(false);
    const error = useEditorStore.getState().openError;
    expect(error?.message).toBe(en['editor.openFailed']);
    // No dictionary answers to the code, so the shared lookup keeps the sentence.
    expect(error?.code).toBe('editorRefused');
    expect(error?.technical).toBe('ffprobe exited with status 1');
  });

  test('a container the app cannot copy into opens ready to re-encode', async () => {
    await openClip('C:/tmp/old.avi', { container: 'avi' });

    const options = selectOptions(useEditorStore.getState())!;
    expect(options.container).toBe('mp4');
    expect(options.mode).toBe('reencode');
  });

  test('a container it can copy into opens as a cut and nothing else', async () => {
    await openClip('C:/tmp/a.mkv', { container: 'mkv' });

    const options = selectOptions(useEditorStore.getState())!;
    expect(options.container).toBe('mkv');
    expect(options.mode).toBe('lossless');
  });
});
