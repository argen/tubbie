<script lang="ts">
  /**
   * Settings — About section (Phase 4). A Settings section, NOT a route and
   * never in the popover (the plan: About lives beside Updates). Mirrors the
   * UpdatesSection structure and reuses the global `settings__*` classes.
   *
   * Version comes from `@tauri-apps/api/app::getVersion`; like UpdatesSection
   * it degrades to "—" if unavailable (e.g. in a plain DOM test without the
   * Tauri app mock) so it never throws.
   */
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { openExternal } from '$lib/ipc/commands.js';

  const SOURCE_URL = 'https://github.com/argen/tubbie';
  const TFL_OPEN_DATA_URL = 'https://tfl.gov.uk/info-for/open-data-users/';

  let version = $state('—');

  onMount(async () => {
    try {
      version = await getVersion();
    } catch {
      version = '—';
    }
  });
</script>

<section class="settings__section" aria-labelledby="section-about">
  <h2 id="section-about" class="settings__section-title">About</h2>

  <p class="settings__api-status" data-testid="about-version">Tubbie {version}</p>

  <p class="settings__api-hint">Live TfL arrivals in your menu bar and on your desktop.</p>

  <div class="about__links">
    <a
      href={SOURCE_URL}
      onclick={(e) => {
        e.preventDefault();
        void openExternal(SOURCE_URL);
      }}
      class="settings__link">Source &amp; releases</a
    >
    <a
      href={TFL_OPEN_DATA_URL}
      onclick={(e) => {
        e.preventDefault();
        void openExternal(TFL_OPEN_DATA_URL);
      }}
      class="settings__link">TfL Open Data</a
    >
  </div>

  <p class="settings__api-hint settings__api-hint--small">
    Powered by TfL Open Data. Contains OS data © Crown copyright and database rights 2016. © 2026
    Bruno Belcastro.
  </p>
</section>

<style>
  /* Shared rules (`.settings__section`, `.settings__section-title`,
     `.settings__api-status`, `.settings__api-hint*`, `.settings__link`) live
     in `SettingsView.svelte` under `:global(...)`. */

  .about__links {
    display: flex;
    gap: 1rem;
    margin: 0.2rem 0 0.4rem;
  }
</style>
