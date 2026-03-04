document.getElementById('search-box').addEventListener('input', (e) => {
    query = e.target.value.toLowerCase();
    render();
});

setInterval(() => {
    const now = new Date();
    const clock = document.getElementById('clock-text');
    if (clock) clock.innerText = now.toLocaleTimeString();

    const currentSeconds = now.getSeconds();
    const nextRefreshSec = REFRESH_INTERVAL_SEC - (currentSeconds % REFRESH_INTERVAL_SEC);
    const timer = document.getElementById('sync-timer');

    if (nextRefreshSec === REFRESH_INTERVAL_SEC && currentSeconds !== lastFetchSec && !isActivelyFetching) {
        lastFetchSec = currentSeconds;
        fetchTelemetry(true);
    }

    if (timer) {
        if (isActivelyFetching) {
            timer.style.display = 'none';
        } else {
            timer.style.display = 'inline-flex';
            const displaySec = String(nextRefreshSec).padStart(2, '0');
            timer.innerText = `[${displaySec}s]`;
        }
    }
}, 1000);

fetchTelemetry(false);
lucide.createIcons();
