<script module lang="ts">
    declare const __VERSION__: string;
    declare const __COMMIT__: string;
    declare const __BUILD_DATE__: string;
</script>

<script lang="ts">
    // Render local date+time; the define injects an ISO-8601 UTC string.
    const buildDate = new Date(__BUILD_DATE__).toLocaleString();
</script>

<div class="about">
    <div class="about-name">Yomeru</div>
    <div class="about-row">
        <span class="about-label">Version</span>
        <span class="about-value">{__VERSION__}</span>
    </div>
    <div class="about-row">
        <span class="about-label">Commit</span>
        <span class="about-value about-commit">{__COMMIT__}</span>
    </div>
    <div class="about-row">
        <span class="about-label">Built</span>
        <span class="about-value">{buildDate}</span>
    </div>
</div>

<style>
    .about {
        display: flex;
        flex-direction: column;
        gap: 12px;
        padding: 8px 0;
    }
    .about-name {
        font-size: 22px;
        font-weight: 700;
        color: var(--blue);
        margin-bottom: 4px;
    }
    .about-row {
        display: flex;
        gap: 12px;
        align-items: baseline;
    }
    .about-label {
        font-size: 12px;
        color: var(--subtext);
        width: 60px;
        flex-shrink: 0;
    }
    .about-value {
        font-size: 14px;
        color: var(--text);
    }
    .about-commit {
        font-family: monospace;
        font-size: 13px;
        color: var(--accent);
    }
</style>
