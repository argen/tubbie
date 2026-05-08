<script lang="ts">
  import {
    settingsForm,
    availableLineIds,
    currentStationName,
    updateForm,
    persistDebounced,
  } from '$lib/stores/settingsForm.js';

  // Master roster of selectable line chips. Tube + DLR + Elizabeth +
  // the six named Overground lines (Mildmay/Lioness/Suffragette/Windrush/
  // Weaver/Liberty — TfL split the Overground in November 2024). The
  // visible/disabled subset for any station is intersected with the
  // station's `Station.lines` field via `availableLineIds`.
  //
  // Elizabeth uses the line-form id `'elizabeth'` (matches
  // `Station.lines[].id` and the wire format after
  // `tfl_domain::canonicalize_line_id` runs at deserialization). The
  // mode-form `'elizabeth-line'` is migrated on config load so any
  // historical config keeps working.
  const KNOWN_LINES: { id: string; label: string }[] = [
    { id: 'bakerloo', label: 'Bakerloo' },
    { id: 'central', label: 'Central' },
    { id: 'circle', label: 'Circle' },
    { id: 'district', label: 'District' },
    { id: 'elizabeth', label: 'Elizabeth' },
    { id: 'hammersmith-city', label: 'Hammersmith & City' },
    { id: 'jubilee', label: 'Jubilee' },
    { id: 'metropolitan', label: 'Metropolitan' },
    { id: 'northern', label: 'Northern' },
    { id: 'piccadilly', label: 'Piccadilly' },
    { id: 'victoria', label: 'Victoria' },
    { id: 'waterloo-city', label: 'Waterloo & City' },
    { id: 'dlr', label: 'DLR' },
    { id: 'liberty', label: 'Liberty' },
    { id: 'lioness', label: 'Lioness' },
    { id: 'mildmay', label: 'Mildmay' },
    { id: 'suffragette', label: 'Suffragette' },
    { id: 'weaver', label: 'Weaver' },
    { id: 'windrush', label: 'Windrush' },
  ];

  function isLineAvailable(lineId: string): boolean {
    return $availableLineIds === null || $availableLineIds.has(lineId);
  }

  function toggleLine(lineId: string): void {
    if (!isLineAvailable(lineId)) return;
    const current = $settingsForm.lineIds;
    const next = current.includes(lineId)
      ? current.filter((id) => id !== lineId)
      : [...current, lineId];
    updateForm({ lineIds: next });
    // Debounce: a 12-chip toggle burst becomes one save_config carrying
    // the final state, instead of 12 disk writes + 12 cfg_tx.send round
    // trips. The flushPending hook in onDestroy / beforeunload makes
    // sure a click made just before closing Settings still saves.
    persistDebounced();
  }
</script>

<section class="settings__section" aria-labelledby="section-lines">
  <h2 id="section-lines" class="settings__section-title">
    Lines
    <span class="settings__section-hint">(empty = all lines)</span>
  </h2>
  <div class="settings__chips" role="group" aria-label="Select lines to filter">
    {#each KNOWN_LINES as line (line.id)}
      {@const available = isLineAvailable(line.id)}
      <button
        type="button"
        class="settings__chip"
        class:settings__chip--selected={$settingsForm.lineIds.includes(line.id)}
        class:settings__chip--unavailable={!available}
        disabled={!available}
        onclick={() => {
          toggleLine(line.id);
        }}
        aria-pressed={$settingsForm.lineIds.includes(line.id)}
        aria-disabled={!available}
        aria-label={available
          ? `Toggle ${line.label} line`
          : `${line.label} line is not served by this station`}
        title={available ? undefined : `Not served by ${$currentStationName || 'this station'}`}
      >
        {line.label}
      </button>
    {/each}
  </div>
</section>
