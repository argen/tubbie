<script lang="ts">
  import type { ThemeId } from '$lib/stores/config.js';

  interface ThemeOption {
    id: ThemeId;
    label: string;
    bg: string;
    fg: string;
  }

  const THEMES: ThemeOption[] = [
    { id: 'classic-amber', label: 'Classic Amber', bg: '#0A0A0A', fg: '#FFB800' },
    { id: 'classic-orange', label: 'Classic Orange', bg: '#0A0A0A', fg: '#FF6B00' },
    { id: 'modern-white', label: 'Modern White', bg: '#0D0D0D', fg: '#F0F0F0' },
    { id: 'high-contrast', label: 'High Contrast', bg: '#000000', fg: '#FFFFFF' },
  ];

  interface Props {
    selected: string;
    onSelect: (themeId: ThemeId) => void;
  }

  const { selected, onSelect }: Props = $props();

  function handleSelect(id: ThemeId): void {
    onSelect(id);
  }
</script>

<fieldset class="theme-picker" aria-label="Select theme">
  <legend class="theme-picker__legend">Theme</legend>
  <div class="theme-picker__swatches" role="group" aria-label="Theme options">
    {#each THEMES as theme (theme.id)}
      <button
        type="button"
        class="theme-picker__swatch"
        class:theme-picker__swatch--selected={selected === theme.id}
        style:background={theme.bg}
        style:color={theme.fg}
        style:border-color={selected === theme.id ? theme.fg : 'transparent'}
        onclick={() => {
          handleSelect(theme.id);
        }}
        aria-pressed={selected === theme.id}
        aria-label="Theme: {theme.label}"
        title={theme.label}
      >
        <span class="theme-picker__swatch-label" style:color={theme.fg}>
          {theme.label}
        </span>
        <span class="theme-picker__swatch-preview" aria-hidden="true" style:color={theme.fg}>
          19:45
        </span>
      </button>
    {/each}
  </div>
</fieldset>

<style>
  .theme-picker {
    border: 1px solid var(--input-border);
    border-radius: 2px;
    padding: 0.75rem;
  }

  .theme-picker__legend {
    font-family: var(--font-board);
    font-size: 0.85rem;
    color: var(--platform-label);
    text-transform: uppercase;
    letter-spacing: 0.1em;
    padding: 0 0.3rem;
  }

  .theme-picker__swatches {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 0.5rem;
  }

  .theme-picker__swatch {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
    padding: 0.6rem 0.5rem;
    border: 2px solid transparent;
    border-radius: 2px;
    cursor: pointer;
    transition: border-color 0.1s ease;
    min-height: 64px;
  }

  .theme-picker__swatch:hover {
    filter: brightness(1.2);
  }

  .theme-picker__swatch:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
  }

  .theme-picker__swatch--selected {
    box-shadow: 0 0 6px currentColor;
  }

  .theme-picker__swatch-label {
    font-family: var(--font-board);
    font-size: 0.75rem;
    letter-spacing: 0.08em;
    text-align: center;
    opacity: 0.8;
  }

  .theme-picker__swatch-preview {
    font-family: 'DSEG14Classic', 'VT323', monospace;
    font-size: 1.1rem;
    letter-spacing: 0.05em;
  }

  @media (prefers-reduced-motion: reduce) {
    .theme-picker__swatch {
      transition: none;
    }
  }
</style>
