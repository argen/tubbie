/**
 * Favorites store — wraps listFavorites / addFavorite / removeFavorite IPC.
 *
 * Mutations bypass the cfg pipeline. Selecting a favorite goes through the
 * existing `updateConfig({ station_id })` path, which triggers an immediate
 * stream refresh per invariant #2.
 */

import { writable, get } from 'svelte/store';
import {
  addFavorite as ipcAddFavorite,
  listFavorites as ipcListFavorites,
  removeFavorite as ipcRemoveFavorite,
} from '$lib/ipc/commands.js';
import type { Favorite, LineRef } from '$lib/ipc/types.js';

export const favorites = writable<Favorite[]>([]);
export const favoritesError = writable<string | null>(null);

/**
 * Load favorites from the Rust backend and populate the store. Call once on
 * settings mount.
 */
export async function initFavorites(): Promise<void> {
  try {
    const list = await ipcListFavorites();
    favorites.set(list);
    favoritesError.set(null);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    favoritesError.set(`Failed to load favorites: ${msg}`);
  }
}

/**
 * Add a station to favorites. Idempotent — a duplicate `station_id` is a
 * no-op (the backend enforces this). Returns the updated list.
 */
export async function addFavorite(
  stationId: string,
  commonName: string,
  lines: LineRef[],
): Promise<void> {
  const previous = get(favorites);
  try {
    const updated = await ipcAddFavorite(stationId, commonName, lines);
    favorites.set(updated);
    favoritesError.set(null);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    favoritesError.set(`Failed to add favorite: ${msg}`);
    favorites.set(previous);
  }
}

/** Remove a favorite by station_id. Returns the updated list. */
export async function removeFavorite(stationId: string): Promise<void> {
  const previous = get(favorites);
  try {
    const updated = await ipcRemoveFavorite(stationId);
    favorites.set(updated);
    favoritesError.set(null);
  } catch (err: unknown) {
    const msg = err instanceof Error ? err.message : String(err);
    favoritesError.set(`Failed to remove favorite: ${msg}`);
    favorites.set(previous);
  }
}

/** True if the given station id is currently in favorites. */
export function isFavoriteStation(stationId: string): boolean {
  return get(favorites).some((f) => f.station_id === stationId);
}
