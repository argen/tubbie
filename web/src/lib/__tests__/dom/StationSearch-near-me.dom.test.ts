// @vitest-environment happy-dom
//
// Coverage for the "find nearest station" affordance added to
// StationSearch.svelte:
//   - Crosshair button is present and labelled
//   - Tapping it shows the ACQUIRING FIX status row
//   - Successful flow renders nearby stations in distance order with
//     a distance chip on each row
//   - PermissionDenied surfaces as a typed error row, not a toast
//   - Retry path on Timeout re-fires the location request
//
// All Tauri IPC is mocked at the `$lib/ipc/commands.js` boundary so we
// never touch the (macOS-only) native bridge.

import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, within } from '@testing-library/svelte';
import StationSearch from '$lib/components/StationSearch.svelte';
import type { LocationError, LocationFix, NearbyStation, Station } from '$lib/ipc/types.js';
import { sampleNearbyStations, sampleLocationFix } from '$lib/ipc/mock.js';

const searchStationsMock = vi.fn(async (_q: string): Promise<Station[]> => []);
const findNearestStationsMock = vi.fn(
  async (_lat: number, _lon: number, _limit: number): Promise<NearbyStation[]> =>
    sampleNearbyStations,
);
const requestCurrentLocationMock = vi.fn(
  async (): Promise<{ ok: true; fix: LocationFix } | { ok: false; error: LocationError }> => ({
    ok: true,
    fix: sampleLocationFix,
  }),
);
const openExternalMock = vi.fn(async (_url: string): Promise<void> => undefined);

vi.mock('$lib/ipc/commands.js', () => ({
  searchStations: (q: string) => searchStationsMock(q),
  findNearestStations: (lat: number, lon: number, limit: number) =>
    findNearestStationsMock(lat, lon, limit),
  requestCurrentLocation: () => requestCurrentLocationMock(),
  openExternal: (url: string) => openExternalMock(url),
}));

describe('StationSearch — near me', () => {
  beforeEach(() => {
    searchStationsMock.mockClear();
    findNearestStationsMock.mockClear();
    requestCurrentLocationMock.mockClear();
    openExternalMock.mockClear();
    requestCurrentLocationMock.mockImplementation(async () => ({
      ok: true,
      fix: sampleLocationFix,
    }));
    findNearestStationsMock.mockImplementation(async () => sampleNearbyStations);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the crosshair button with an accessible name', () => {
    render(StationSearch, { props: { selectedId: '', onSelect: vi.fn() } });
    const btn = screen.getByRole('button', { name: /find nearest stations/i });
    expect(btn).toBeTruthy();
  });

  it('shows ACQUIRING FIX while the location request is in flight', async () => {
    let resolveLocation!: (
      v: { ok: true; fix: LocationFix } | { ok: false; error: LocationError },
    ) => void;
    requestCurrentLocationMock.mockImplementationOnce(
      () =>
        new Promise<{ ok: true; fix: LocationFix } | { ok: false; error: LocationError }>(
          (resolve) => {
            resolveLocation = resolve;
          },
        ),
    );

    render(StationSearch, { props: { selectedId: '', onSelect: vi.fn() } });
    const btn = screen.getByRole('button', { name: /find nearest stations/i });
    await fireEvent.mouseDown(btn);

    const status = await screen.findByTestId('station-search-locating');
    expect(status.textContent ?? '').toMatch(/ACQUIRING FIX/i);

    // Let the test finish cleanly.
    resolveLocation({ ok: true, fix: sampleLocationFix });
  });

  it('renders nearby stations with distance chips, ordered by distance', async () => {
    const onSelect = vi.fn();
    render(StationSearch, { props: { selectedId: '', onSelect } });
    const btn = screen.getByRole('button', { name: /find nearest stations/i });
    await fireEvent.mouseDown(btn);

    // The first nearby station is "Bank" (closest). Wait for it.
    const listbox = await screen.findByRole('listbox', { name: /nearest stations/i });
    const items = within(listbox).getAllByRole('option');
    expect(items.length).toBe(sampleNearbyStations.length);
    // The closest is rendered first.
    expect(items[0]?.getAttribute('aria-label') ?? '').toMatch(/Bank/);
    // Each row has a distance chip.
    const distances = listbox.querySelectorAll('.station-search__result-distance');
    expect(distances.length).toBe(sampleNearbyStations.length);
    // The "0 m" closest row renders some non-empty distance label
    // (per the formatter: 0 → "0M" or "0.00MI" depending on locale).
    expect((distances[0]?.textContent ?? '').trim().length).toBeGreaterThan(0);

    // Selecting a row invokes onSelect with the underlying station.
    const second = items[1];
    if (second) {
      await fireEvent.mouseDown(second);
      expect(onSelect).toHaveBeenCalledWith(sampleNearbyStations[1]?.station);
    }
  });

  it('renders a typed listbox row for PermissionDenied — never a toast', async () => {
    requestCurrentLocationMock.mockImplementationOnce(async () => ({
      ok: false,
      error: { kind: 'PermissionDenied' },
    }));

    render(StationSearch, { props: { selectedId: '', onSelect: vi.fn() } });
    const btn = screen.getByRole('button', { name: /find nearest stations/i });
    await fireEvent.mouseDown(btn);

    const errorRow = await screen.findByTestId('station-search-location-error');
    expect(errorRow.getAttribute('data-error-kind')).toBe('PermissionDenied');
    // Denied/off permission must point the user at System Settings, NOT imply a
    // transient signal problem ("NO SIGNAL — TRY AGAIN") — macOS won't re-prompt.
    expect(errorRow.textContent ?? '').toMatch(/OPEN SETTINGS/i);
    expect(errorRow.textContent ?? '').not.toMatch(/NO SIGNAL/i);
    // The error must NOT be a `role="alert"` toast — it lives inside the
    // listbox alongside the rest of the search affordance.
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('clicking a permission-denied row opens the Location Services settings pane', async () => {
    requestCurrentLocationMock.mockImplementationOnce(async () => ({
      ok: false,
      error: { kind: 'PermissionDenied' },
    }));

    render(StationSearch, { props: { selectedId: '', onSelect: vi.fn() } });
    const btn = screen.getByRole('button', { name: /find nearest stations/i });
    await fireEvent.mouseDown(btn);

    const errorRow = await screen.findByTestId('station-search-location-error');
    await fireEvent.mouseDown(errorRow);
    // Opens the macOS Location Services pane (does NOT retry — that would just
    // time out again, since the permission is decided).
    expect(openExternalMock).toHaveBeenCalledWith(
      'x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices',
    );
    expect(requestCurrentLocationMock).toHaveBeenCalledTimes(1);
  });

  it('Timeout error row re-fires the request when clicked', async () => {
    // First call: timeout. Second call (the retry): success.
    requestCurrentLocationMock
      .mockImplementationOnce(async () => ({ ok: false, error: { kind: 'Timeout' } }))
      .mockImplementationOnce(async () => ({ ok: true, fix: sampleLocationFix }));

    render(StationSearch, { props: { selectedId: '', onSelect: vi.fn() } });
    const btn = screen.getByRole('button', { name: /find nearest stations/i });
    await fireEvent.mouseDown(btn);

    const errorRow = await screen.findByTestId('station-search-location-error');
    expect(errorRow.getAttribute('data-error-kind')).toBe('Timeout');

    // Click the row → retry. After the retry resolves, the listbox should
    // contain nearby-station rows.
    await fireEvent.mouseDown(errorRow);
    const listbox = await screen.findByRole('listbox', { name: /nearest stations/i });
    expect(within(listbox).getAllByRole('option').length).toBe(sampleNearbyStations.length);
    expect(requestCurrentLocationMock).toHaveBeenCalledTimes(2);
  });

  it('typing in the input exits near-me mode (no nearby listbox visible)', async () => {
    render(StationSearch, { props: { selectedId: '', onSelect: vi.fn() } });
    const btn = screen.getByRole('button', { name: /find nearest stations/i });
    await fireEvent.mouseDown(btn);
    await screen.findByRole('listbox', { name: /nearest stations/i });

    const input = screen.getByRole('combobox');
    await fireEvent.input(input, { target: { value: 'Bel' } });
    // The nearby listbox should be gone immediately on input.
    expect(screen.queryByRole('listbox', { name: /nearest stations/i })).toBeNull();
  });
});
