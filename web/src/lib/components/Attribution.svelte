<script lang="ts">
  // Attribution is required by TfL Open Data terms of service.
  // Link: https://tfl.gov.uk/corporate/terms-and-conditions/transport-data-service
  import { openExternal } from '$lib/ipc/commands.js';

  const TFL_TERMS_URL = 'https://tfl.gov.uk/corporate/terms-and-conditions/transport-data-service';
</script>

<footer class="attribution">
  <span class="attribution__text">
    Powered by
    <!-- href is kept for semantics (copy-link, screen readers) but the click is
         routed through the opener plugin — a bare target="_blank" is a no-op in
         a WKWebView. -->
    <a
      href={TFL_TERMS_URL}
      onclick={(e) => {
        e.preventDefault();
        void openExternal(TFL_TERMS_URL);
      }}
      aria-label="TfL Open Data (opens in your browser)"
      class="attribution__link"
    >
      TfL Open Data
    </a>
  </span>
</footer>

<style>
  .attribution {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg);
    border-top: 1px solid var(--row-divider);
    z-index: 50;
  }

  .attribution__text {
    font-family: var(--font-board);
    font-size: 13px;
    color: var(--platform-label);
    letter-spacing: 0.05em;
    opacity: 0.7;
  }

  .attribution__link {
    color: var(--platform-label);
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .attribution__link:hover,
  .attribution__link:focus {
    color: var(--fg);
    opacity: 1;
  }
</style>
