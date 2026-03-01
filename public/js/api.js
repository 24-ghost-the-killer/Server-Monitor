async function fetchTelemetry(showVisuals = true) {
    const startTime = Date.now();
    const engineBadge = document.getElementById('engine-status');
    const syncIcon = document.getElementById('sync-icon');
    const syncText = document.getElementById('sync-text');
    const timer = document.getElementById('sync-timer');

    isActivelyFetching = showVisuals;

    if (showVisuals) {
        if (engineBadge) {
            engineBadge.classList.add('refreshing');
            engineBadge.classList.remove('synced');
            engineBadge.classList.add('syncing');
        }
        if (syncText) syncText.innerText = "MESH: PULLING TELEMETRY";
        if (timer) timer.style.display = 'none';
    }

    try {
        const res = await fetch('api/stats');
        if (!res.ok) throw new Error('API request failed');
        const data = await res.json();
        telemetry = data;

        const hostsMap = {};
        data.forEach(d => {
            const addr = d.target_address || "unknown";
            if (!hostsMap[addr]) hostsMap[addr] = [];
            hostsMap[addr].push(d);
        });

        let healthyNodes = 0, failedNodes = 0, syncingNodes = 0;
        Object.keys(hostsMap).forEach(ip => {
            const checks = hostsMap[ip];
            const isSyncing = checks.some(c => (c.message || "").includes("Awaiting"));
            const isAllUp = checks.every(c => c.status);
            if (isSyncing) syncingNodes++;
            else if (isAllUp) healthyNodes++;
            else failedNodes++;
        });

        const responders = data.filter(d => getLat(d) > 0);
        const sumLat = responders.reduce((acc, curr) => acc + getLat(curr), 0);
        const avgLat = responders.length > 0 ? (sumLat / responders.length) : 0;

        safeSetText('stat-total', Object.keys(hostsMap).length);
        safeSetText('stat-up', healthyNodes);
        safeSetText('stat-down', failedNodes);
        safeSetText('stat-up-badge', healthyNodes);
        safeSetText('stat-down-badge', failedNodes);
        safeSetText('stat-latency', responders.length > 0 ? avgLat.toFixed(1) + 'ms' : '--');

        render();
        safeSetText('last-sync-time', new Date().toLocaleTimeString());

        const finalSyncText = syncingNodes > 0 ? `MESH: SYNCHRONIZING (${syncingNodes})` : 'MESH: SYNCHRONIZED';
        const isSyncing = syncingNodes > 0;

        const syncSuccess = () => {
            if (engineBadge && syncText && syncIcon) {
                if (isSyncing) {
                    engineBadge.classList.replace('synced', 'syncing');
                    syncText.innerText = finalSyncText;
                } else {
                    engineBadge.classList.replace('syncing', 'synced');
                    syncText.innerText = finalSyncText;
                }
                syncIcon.classList.remove('spinning');
            }
        };

        const elapsed = Date.now() - startTime;
        const minimumDuration = showVisuals ? 3000 : 0;
        const remaining = Math.max(0, minimumDuration - elapsed);

        setTimeout(() => {
            isActivelyFetching = false;
            syncSuccess();
            if (engineBadge) engineBadge.classList.remove('refreshing');

            if (timer) {
                const now = new Date();
                const curSec = now.getSeconds();
                const nextSec = REFRESH_INTERVAL_SEC - (curSec % REFRESH_INTERVAL_SEC);
                timer.innerText = `[${String(nextSec).padStart(2, '0')}s]`;
                timer.style.display = 'inline-flex';
            }
        }, remaining);

    } catch (err) {
        console.error("Failed to fetch server data", err);
        setTimeout(() => {
            isActivelyFetching = false;
            if (syncText) syncText.innerText = "MESH: CONNECTION FAULT";
            if (engineBadge) engineBadge.classList.remove('refreshing');
            if (timer) timer.style.display = 'inline-flex';
        }, 1000);
    }
}
