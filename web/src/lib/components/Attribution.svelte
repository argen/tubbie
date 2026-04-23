<script lang="ts">
  // Attribution is required by TfL Open Data terms of service.
  // Link: https://tfl.gov.uk/corporate/terms-and-conditions/transport-data-service
</script>

<footer class="attribution">
  <span class="attribution__text">
    Powered by
    <a
      href="https://tfl.gov.uk/corporate/terms-and-conditions/transport-data-service"
      target="_blank"
      rel="noopener noreferrer"
      aria-label="TfL Open Data (opens in new tab)"
      class="attribution__link"
    >
      TfL Open Data
    </a>
  </span>
</footer>

<style>
  .attribution {
    position: fixed;
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
    font-family: 'VT323', monospace;
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
