/**
 * `BoardService.refresh` — the per-tick board pipeline, ported from
 * `crates/tfl-board/src/service.rs`. One arrivals fetch through the filter chain
 * into a grouped `Board`. The polling stream that drives `refresh` on a timer
 * (with the config-diff / lifecycle / last-ok logic) lands as `BoardStream` in a
 * follow-up; this is the pure-ish core it calls.
 */

import type { Board, BoardConfig } from '$lib/ipc/types.js';
import type { Clock } from '../transport/clock.js';
import { SystemClock } from '../transport/clock.js';
import type { TflClient } from '../cache/tflClient.js';
import {
  applyFilters,
  dropArrivalsForLinesNotServing,
  dropArrivalsTerminatingAtQueriedStation,
  dropOffAxisPredictions,
} from './filters.js';
import { buildBoard } from './buildBoard.js';

export class BoardService {
  private readonly client: TflClient;
  private readonly clock: Clock;

  constructor(client: TflClient, clock: Clock = new SystemClock()) {
    this.client = client;
    this.clock = clock;
  }

  /**
   * Fetch and group the board for `cfg.station_id`: arrivals → directions
   * filter → not-serving filter (#10) → off-axis filter → terminating filter
   * (#24) → `buildBoard`. `generated_at` is stamped from the injected clock;
   * `stale_since` is always `null` on a successful refresh (the stream sets it
   * when re-emitting a stale board after a failure).
   */
  async refresh(cfg: BoardConfig): Promise<Board> {
    const raw = await this.client.getArrivals(cfg.station_id);
    const allowed = this.client.allowedLineIdsFor(cfg.station_id);

    let arrivals = applyFilters(raw, cfg);
    arrivals = dropArrivalsForLinesNotServing(allowed, cfg.station_id, arrivals);
    arrivals = dropOffAxisPredictions(arrivals);
    arrivals = dropArrivalsTerminatingAtQueriedStation(arrivals);

    return buildBoard(cfg.station_id, arrivals, this.clock.now(), null);
  }
}
