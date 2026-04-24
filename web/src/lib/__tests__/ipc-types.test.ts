import { describe, expect, it } from 'vitest';
import {
  isBoard,
  isBoardConfig,
  isLineStatus,
  isStation,
  type Arrival,
  type Board,
  type BoardConfig,
  type LineStatus,
  type Station,
} from '$lib/ipc/types.js';
import { sampleBoard, sampleConfig, sampleLineStatus, sampleStation } from '$lib/ipc/mock.js';

describe('IPC type guards', () => {
  // -----------------------------------------------------------------------
  // isBoard
  // -----------------------------------------------------------------------

  it('isBoard accepts a valid Board', () => {
    expect(isBoard(sampleBoard)).toBe(true);
  });

  it('isBoard accepts a Board with non-null stale_since', () => {
    const stale: Board = { ...sampleBoard, stale_since: '2025-01-15T10:00:00Z' };
    expect(isBoard(stale)).toBe(true);
  });

  it('isBoard rejects null', () => {
    expect(isBoard(null)).toBe(false);
  });

  it('isBoard rejects an array', () => {
    expect(isBoard([])).toBe(false);
  });

  it('isBoard rejects a Board missing station_id', () => {
    const bad = { ...sampleBoard, station_id: undefined };
    expect(isBoard(bad)).toBe(false);
  });

  it('isBoard rejects a Board with invalid stale_since type', () => {
    const bad = { ...sampleBoard, stale_since: 42 };
    expect(isBoard(bad)).toBe(false);
  });

  // -----------------------------------------------------------------------
  // isStation
  // -----------------------------------------------------------------------

  it('isStation accepts a valid Station', () => {
    expect(isStation(sampleStation)).toBe(true);
  });

  it('isStation rejects null', () => {
    expect(isStation(null)).toBe(false);
  });

  it('isStation rejects an object missing common_name', () => {
    const bad: Omit<Station, 'common_name'> & { common_name?: string } = { ...sampleStation };
    delete bad.common_name;
    expect(isStation(bad)).toBe(false);
  });

  // -----------------------------------------------------------------------
  // isBoardConfig
  // -----------------------------------------------------------------------

  it('isBoardConfig accepts a valid BoardConfig', () => {
    expect(isBoardConfig(sampleConfig)).toBe(true);
  });

  it('isBoardConfig rejects non-array directions', () => {
    const bad = { ...sampleConfig, directions: 'Northbound' };
    expect(isBoardConfig(bad)).toBe(false);
  });

  it('isBoardConfig rejects non-number poll_seconds', () => {
    const bad = { ...sampleConfig, poll_seconds: '20' };
    expect(isBoardConfig(bad)).toBe(false);
  });

  // -----------------------------------------------------------------------
  // isLineStatus
  // -----------------------------------------------------------------------

  it('isLineStatus accepts a valid LineStatus', () => {
    expect(isLineStatus(sampleLineStatus)).toBe(true);
  });

  it('isLineStatus accepts null disruption_text', () => {
    const noDisruption: LineStatus = { ...sampleLineStatus, disruption_text: null };
    expect(isLineStatus(noDisruption)).toBe(true);
  });

  it('isLineStatus rejects number disruption_text', () => {
    const bad = { ...sampleLineStatus, disruption_text: 42 };
    expect(isLineStatus(bad)).toBe(false);
  });

  // -----------------------------------------------------------------------
  // Fixture shape (proves each type has a valid sample)
  // -----------------------------------------------------------------------

  it('sampleBoard has correct shape', () => {
    const b: Board = sampleBoard;
    expect(b.station_id).toBe('940GZZLUBZP');
    expect(b.stale_since).toBeNull();
    expect(Array.isArray(b.platforms)).toBe(true);
  });

  it('sampleStation has correct shape', () => {
    const s: Station = sampleStation;
    expect(s.id).toBe('940GZZLUBZP');
    expect(s.modes).toContain('tube');
  });

  it('sampleConfig has theme field', () => {
    const c: BoardConfig = sampleConfig;
    expect(c.theme).toBe('classic-amber');
  });

  it('sampleLineStatus has correct shape', () => {
    const ls: LineStatus = sampleLineStatus;
    expect(ls.line_id).toBe('northern');
    expect(ls.disruption_text).toBeNull();
  });

  it('sample arrival has required fields', () => {
    const arr: Arrival = sampleBoard.platforms[0]!.arrivals[0]!;
    expect(typeof arr.id).toBe('string');
    expect(typeof arr.time_to_station).toBe('number');
    expect(typeof arr.destination_name).toBe('string');
  });
});

describe('IPC type — no @html usage', () => {
  // Security: assert no source files in src/lib/ use {@html} with TfL strings.
  // This is a static analysis smoke test — it reads source text at test time.
  it('no Svelte component uses {@html} in src/lib/components/', async () => {
    // Read component sources via dynamic import of raw text using ?raw suffix
    // In Vite, ?raw imports work. We grep for the pattern.
    const { readFileSync, readdirSync } = await import('node:fs');
    const { join } = await import('node:path');
    const { fileURLToPath } = await import('node:url');

    const dir = join(fileURLToPath(new URL('.', import.meta.url)), '../../lib/components');
    const files = readdirSync(dir).filter((f) => f.endsWith('.svelte'));

    for (const file of files) {
      const content = readFileSync(join(dir, file), 'utf-8');
      // {@html ...} with TfL-sourced data is banned
      // We check no unguarded {@html} exists at all
      expect(content, `${file} must not use {@html}`).not.toMatch(/\{@html/);
    }
  });
});
