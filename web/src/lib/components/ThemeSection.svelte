<script lang="ts">
  import ThemePicker from '$lib/components/ThemePicker.svelte';
  import { applyTheme, type ThemeId } from '$lib/stores/config.js';
  import { settingsForm, updateForm, persistDebounced } from '$lib/stores/settingsForm.js';

  function handleThemeSelect(newTheme: ThemeId): void {
    updateForm({ theme: newTheme });
    // Live preview — apply to DOM immediately, then debounce the persist.
    // The user sees the theme change instantly; the disk write coalesces
    // if they tap through several themes in quick succession.
    applyTheme(newTheme);
    persistDebounced();
  }
</script>

<section class="settings__section" aria-labelledby="section-theme">
  <h2 id="section-theme" class="settings__section-title">Theme</h2>
  <ThemePicker selected={$settingsForm.theme} onSelect={handleThemeSelect} />
</section>
