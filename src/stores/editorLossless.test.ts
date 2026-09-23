/**
 * The rule that keeps the mode and the settings from disagreeing: a lossless
 * copy is only on offer while nothing it cannot do has been asked for, and a
 * change that asks for such a thing moves the mode off lossless in the same
 * step -- whichever tab the change was made on.
 */

import { beforeEach, describe, expect, test, vi } from 'vitest';

import type { ExportOptions, MediaProbe } from '@/types';

const backend = vi.hoisted(() => ({
  probe: null as unknown as MediaProbe,
}));

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
}));

vi.mock('@/services/ipc', () => ({
  probeMedia: async () => backend.probe,
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

const { losslessBlocker, selectLosslessBlocker, selectOptions, useEditorStore, withLegalMode } =
  await import('@/stores/useEditorStore');

function probeOf(patch: Partial<MediaProbe> = {}): MediaProbe {
  return {
    path: 'C:/tmp/a.mp4',
    fileName: 'a.mp4',
    container: 'mp4',
    sizeBytes: 21_000_000,
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

async function openLossless(): Promise<void> {
  backend.probe = probeOf();
  await useEditorStore.getState().open('C:/tmp/a.mp4', false);
  expect(selectOptions(useEditorStore.getState())!.mode).toBe('lossless');
}

function options(): ExportOptions {
  return selectOptions(useEditorStore.getState())!;
}

beforeEach(() => {
  useEditorStore.getState().closeAll();
});

describe('what rules a lossless copy out', () => {
  test('a new clip in a copyable box has nothing in the way', async () => {
    await openLossless();
    expect(selectLosslessBlocker(useEditorStore.getState())).toBeNull();
  });

  test('a level off 100 % does, and a level on it does not', async () => {
    await openLossless();
    const base = options();
    expect(losslessBlocker({ ...base, volume: 1.5 }, probeOf())).toBe('volume');
    expect(losslessBlocker({ ...base, volume: 0 }, probeOf())).toBe('volume');
    // A slider parked on 100 must not rule anything out over rounding.
    expect(losslessBlocker({ ...base, volume: 1.004 }, probeOf())).toBeNull();
  });

  test('a custom rate does not: a copy has no rate to set', async () => {
    await openLossless();
    expect(losslessBlocker({ ...options(), videoBitrateKbps: 8000 }, probeOf())).toBeNull();
  });

  test('removing the audio does not: a copy can leave a track out', async () => {
    await openLossless();
    expect(losslessBlocker({ ...options(), mute: true }, probeOf())).toBeNull();
  });

  test('a level on sound that is not written does not either', async () => {
    await openLossless();
    const base = options();
    // Muted, the slider keeps its value but nothing is played at it.
    expect(losslessBlocker({ ...base, mute: true, volume: 1.5 }, probeOf())).toBeNull();
    // A silent source has no sound to be louder.
    const silent = probeOf({ hasAudio: false, audioCodec: null, audioBitrateKbps: null });
    expect(losslessBlocker({ ...base, volume: 1.5 }, silent)).toBeNull();
  });
});

describe('the mode follows the settings', () => {
  test('turning the level up leaves lossless in the same step', async () => {
    await openLossless();

    useEditorStore.getState().setOptions({ volume: 1.5 });

    expect(options().volume).toBe(1.5);
    expect(options().mode).toBe('reencode');
  });

  test('removing the audio stays lossless', async () => {
    await openLossless();

    useEditorStore.getState().setOptions({ mute: true });

    expect(options().mute).toBe(true);
    expect(options().mode).toBe('lossless');
  });

  test('so does another shape of frame', async () => {
    await openLossless();

    useEditorStore.getState().setAspect('1:1', 'fill');

    expect(options().aspect).toBe('1:1');
    expect(options().mode).toBe('reencode');
  });

  test('and another box', async () => {
    await openLossless();

    useEditorStore.getState().setOptions({ container: 'mkv' });

    expect(options().mode).toBe('reencode');
  });

  test('putting the setting back leaves the mode where the user can see it', async () => {
    await openLossless();
    useEditorStore.getState().setOptions({ volume: 1.5 });

    useEditorStore.getState().setOptions({ volume: 1 });

    expect(options().mode).toBe('reencode');
    expect(selectLosslessBlocker(useEditorStore.getState())).toBeNull();
  });

  test('a choice lossless can live with leaves it alone', async () => {
    await openLossless();

    useEditorStore.getState().setOptions({ hardware: true });

    expect(options().mode).toBe('lossless');
  });

  test('asking for lossless while something rules it out is refused', () => {
    const probe = probeOf();
    const blocked: ExportOptions = {
      mode: 'lossless',
      container: 'mp4',
      videoCodec: 'h264',
      quality: 'balanced',
      videoBitrateKbps: null,
      fps: 24,
      maxHeight: null,
      aspect: 'source',
      fit: 'fill',
      mute: false,
      volume: 1,
      audioCodec: 'aac',
      audioBitrateKbps: 192,
      toneMapSdr: false,
      hardware: false,
    };
    expect(withLegalMode(blocked, probe).mode).toBe('reencode');
    expect(withLegalMode({ ...blocked, fps: null }, probe).mode).toBe('lossless');
  });
});
