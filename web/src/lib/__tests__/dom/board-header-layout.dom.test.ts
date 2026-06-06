// @vitest-environment happy-dom
/**
 * DOM tests for the Board header reshuffle (designer-led).
 *
 * Menubar mode (fixed ~380px popover): the station name gets its own
 * full-width row with a length-scaled font (so long names don't truncate to
 * "TOTTE…"); the clock moves to its own row and keeps seconds. Window mode
 * keeps the single row and the large name tier.
 */
import { describe, expect, it, afterEach, vi } from 'vitest';
import { render } from '@testing-library/svelte';
import Board from '$lib/components/Board.svelte';
import { sampleBoard } from '$lib/ipc/mock.js';
import { displayMode } from '$lib/stores/displayMode.js';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: () => Promise.resolve(undefined),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: () => Promise.resolve(() => undefined),
}));
vi.mock('$lib/stores/board.js', () => ({
  lastUpdateTs: {
    subscribe: (fn: (v: number) => void) => {
      fn(0);
      return () => undefined;
    },
  },
}));
vi.mock('$lib/stores/reducedMotion.js', () => ({
  reducedMotion: {
    subscribe: (fn: (v: boolean) => void) => {
      fn(false);
      return () => undefined;
    },
  },
}));
vi.mock('$lib/stores/settingsForm.js', () => ({
  selectStation: vi.fn(),
}));

// A long station name that would truncate badly in the old single-row header.
// shortStationName strips " Underground Station" → "Heathrow Terminals 2 & 3"
// → uppercased = 24 chars → the smallest tier.
const LONG_NAME = 'Heathrow Terminals 2 & 3 Underground Station';
const LONG_DISPLAY = 'HEATHROW TERMINALS 2 & 3';

afterEach(() => {
  displayMode.set('window');
});

describe('Board header — menubar reshuffle', () => {
  it('menubar mode: header is two-row and the long name uses the small tier (no cap)', () => {
    displayMode.set('menubar');
    render(Board, { props: { board: sampleBoard, stationName: LONG_NAME } });

    const header = document.querySelector('.board__header');
    expect(header?.classList.contains('board__header--menubar')).toBe(true);

    const name = document.querySelector('.board__station-name');
    expect(name?.textContent?.trim()).toBe(LONG_DISPLAY);
    // Length-scaled to the smallest tier so it fits its full-width row.
    expect(name?.classList.contains('board__station-name--sm')).toBe(true);
  });

  it('menubar mode: clock keeps seconds (on its own row, no width competition)', () => {
    displayMode.set('menubar');
    render(Board, { props: { board: sampleBoard, stationName: LONG_NAME } });

    const clock = document.querySelector('.board__clock');
    expect(clock?.textContent?.trim() ?? '').toMatch(/^\d{1,2}:\d{2}:\d{2}$/); // HH:MM:SS
  });

  it('window mode: single row, large name tier, clock keeps seconds', () => {
    displayMode.set('window');
    render(Board, { props: { board: sampleBoard, stationName: LONG_NAME } });

    const header = document.querySelector('.board__header');
    expect(header?.classList.contains('board__header--menubar')).toBe(false);

    const name = document.querySelector('.board__station-name');
    // Window mode has the width, so the name is always the large tier.
    expect(name?.classList.contains('board__station-name--lg')).toBe(true);

    const clock = document.querySelector('.board__clock');
    expect(clock?.textContent?.trim() ?? '').toMatch(/^\d{1,2}:\d{2}:\d{2}$/); // HH:MM:SS
  });
});
