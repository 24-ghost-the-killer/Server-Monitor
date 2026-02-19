let telemetry = [];
let expandedNodes = new Set(['section-operational', 'section-degraded', 'initialized']);
let query = "";
const REFRESH_INTERVAL_SEC = 30;
let lastFetchSec = -1;
let isActivelyFetching = false;

document.getElementById('search-box').addEventListener('input', (e) => {
    query = e.target.value.toLowerCase();
    render();
});



function safeSetText(id, text) {
    const el = document.getElementById(id);
    if (el) el.innerText = text;
}

function compareIPs(a, b) {
    const aParts = (a || "").split('.').map(Number);
    const bParts = (b || "").split('.').map(Number);
    for (let i = 0; i < 4; i++) {
        if ((aParts[i] || 0) < (bParts[i] || 0)) return -1;
        if ((aParts[i] || 0) > (bParts[i] || 0)) return 1;
    }
    return 0;
}

function getLat(check) {
    if (typeof check.latency_ms === 'number' && check.latency_ms > 0) return check.latency_ms;
    const match = (check.message || "").match(/\[(\d+\.?\d*)ms\]/);
    if (match) return parseFloat(match[1]);
    return 0;
}

function render() {
    const onlineContainer = document.getElementById('online-categories-container');
    const offlineContainer = document.getElementById('offline-categories-container');
    if (!onlineContainer || !offlineContainer) return;

    if (telemetry.length === 0) {
        const loading = '<div style="padding: 1.5rem; color: var(--foreground-subtle);">Synchronizing infra...</div>';
        onlineContainer.innerHTML = loading;
        offlineContainer.innerHTML = loading;
        return;
    }

    const filtered = telemetry.filter(d =>
        (d.category || "").toLowerCase().includes(query) ||
        (d.server_name || "").toLowerCase().includes(query) ||
        (d.target_address || "").toLowerCase().includes(query) ||
        (d.check_type || "").toLowerCase().includes(query)
    );

    const upItems = filtered.filter(d => d.status);
    const downItems = filtered.filter(d => !d.status);

    toggleSectionContent('online-categories-container', 'section-operational', 'chevron-operational');
    toggleSectionContent('offline-categories-container', 'section-degraded', 'chevron-degraded');

    onlineContainer.innerHTML = renderTree(upItems, 'up');
    offlineContainer.innerHTML = renderTree(downItems, 'down');

    lucide.createIcons();
}

function toggleSectionContent(containerId, sectionKey, chevronId) {
    const container = document.getElementById(containerId);
    const chevron = document.getElementById(chevronId);
    const isExpanded = expandedNodes.has(sectionKey);
    if (container) container.style.display = isExpanded ? 'block' : 'none';
    if (chevron) {
        if (isExpanded) chevron.classList.add('open');
        else chevron.classList.remove('open');
    }
}

function renderTree(items, sectionId) {
    const categoryGroups = {};
    items.forEach(d => {
        const cat = d.category || "Uncategorized";
        if (!categoryGroups[cat]) categoryGroups[cat] = {};
        // Group by BOTH server_name and parent_address to ensure subnets stay together properly
        const groupKey = `${d.server_name}||${d.parent_address || ""}`;
        if (!categoryGroups[cat][groupKey]) categoryGroups[cat][groupKey] = [];
        categoryGroups[cat][groupKey].push(d);
    });

    const sortedCategories = Object.keys(categoryGroups).sort();
    let html = '';

    sortedCategories.forEach(catName => {
        const groups = categoryGroups[catName];
        const groupKeys = Object.keys(groups).sort();
        const catId = `cat-${sectionId}-${catName.replace(/\s+/g, '-').toLowerCase()}`;
        const isCatExpanded = expandedNodes.has(catId) || !expandedNodes.has('initialized');

        html += `
            <div class="category-block" style="margin-bottom: 2px;">
                <div class="tbl-row category-header" onclick="toggleNode('${catId}')">
                    <div class="tbl-col tbl-col--name">
                        <div class="node-name-cell" style="font-weight: 700;">
                            <i data-lucide="chevron-right" size="11" class="chevron ${isCatExpanded ? 'open' : ''}" style="margin-right: 8px;"></i>
                            ${catName.toUpperCase()}
                        </div>
                    </div>
                </div>
                <div class="category-content" style="display: ${isCatExpanded ? 'block' : 'none'}">
                    <div class="tbl-head">
                        <div class="tbl-col tbl-col--name">Infrastructure Point</div>
                        <div class="tbl-col tbl-col--status">Status</div>
                        <div class="tbl-col tbl-col--latency">Metric</div>
                        <div class="tbl-col tbl-col--detail">Details</div>
                    </div>
                    <div class="tbl-body">
                        ${groupKeys.map(gKey => {
            const [sName, pAddr] = gKey.split('||');
            return renderServerGroup(sName, pAddr, groups[gKey], catId);
        }).join('')}
                    </div>
                </div>
            </div>
        `;
    });

    return html;
}

function renderServerGroup(sName, parentAddr, items, catId) {
    // Detect true subnets: must have a slash and not be a /32 single host
    const isSubnet = parentAddr.includes("/") && !parentAddr.endsWith("/32");

    const groupKey = `group:${catId}:${sName}:${parentAddr}`;
    const isExpanded = expandedNodes.has(groupKey);

    const clusterStatus = items.every(i => i.status);
    const clusterPings = items.map(i => getLat(i)).filter(l => l > 0);
    const avgLatency = clusterPings.length > 0 ? (clusterPings.reduce((a, c) => a + c, 0) / clusterPings.length).toFixed(1) + 'ms' : '--';
    let status = clusterStatus ? "online" : "degraded";

    // Header displays the subnet/range (parentAddr) as requested
    const displayName = `${sName} <span style="opacity: 0.5; font-size: 0.9em;">(${parentAddr})</span>`;

    let html = `
        <div class="tbl-row tbl-row--server" onclick="event.stopPropagation(); toggleNode('${groupKey}')">
            <div class="tbl-col tbl-col--name">
                <div class="node-name-cell">
                    <button class="expand-btn">
                        <i data-lucide="chevron-right" size="10" class="chevron ${isExpanded ? 'open' : ''}"></i>
                    </button>
                    <span class="node-name-text">${displayName}</span>
                </div>
            </div>
            <div class="tbl-col tbl-col--status">
                <span class="status-badge ${status}"><span class="status-dot ${status}"></span>${status.toUpperCase()}</span>
            </div>
            <div class="tbl-col tbl-col--latency"><span class="latency-value">${avgLatency}</span></div>
            <div class="tbl-col tbl-col--detail">
                <span style="color: hsl(var(--foreground-subtle));">
                    ${isSubnet ? `${new Set(items.map(i => i.target_address)).size} nodes in subnet` : "Dedicated Node"}
                </span>
            </div>
        </div>
    `;

    if (isExpanded) {
        if (isSubnet) {
            // Group by target_address for subnets to create the secondary IP tree level
            const ipGroups = {};
            items.forEach(item => {
                if (!ipGroups[item.target_address]) ipGroups[item.target_address] = [];
                ipGroups[item.target_address].push(item);
            });

            const sortedIps = Object.keys(ipGroups).sort(compareIPs);
            sortedIps.forEach(ip => {
                const ipItems = ipGroups[ip];
                const ipKey = `ip:${groupKey}:${ip}`;
                const isIpExpanded = expandedNodes.has(ipKey);
                const ipStatus = ipItems.every(i => i.status) ? "online" : "offline";

                // Calculate average latency for this specific node
                const ipPings = ipItems.map(i => getLat(i)).filter(l => l > 0);
                const ipAvgLat = ipPings.length > 0 ? (ipPings.reduce((a, c) => a + c, 0) / ipPings.length).toFixed(1) + 'ms' : '--';

                html += `
                    <div class="tbl-row tbl-row--ip-group tbl-row--indent1" onclick="event.stopPropagation(); toggleNode('${ipKey}')">
                        <div class="tbl-col tbl-col--name">
                            <div class="node-name-cell">
                                <button class="expand-btn">
                                    <i data-lucide="chevron-right" size="10" class="chevron ${isIpExpanded ? 'open' : ''}"></i>
                                </button>
                                <span class="node-name-text">${ip}</span>
                            </div>
                        </div>
                        <div class="tbl-col tbl-col--status">
                            <span class="status-badge ${status}"><span class="status-dot ${status}"></span>${status.toUpperCase()}</span>
                        </div>
                        <div class="tbl-col tbl-col--latency">
                            <span class="latency-value" style="opacity: 0.8;">${ipAvgLat}</span>
                        </div>
                        <div class="tbl-col tbl-col--detail"></div>
                    </div>
                `;

                if (isIpExpanded) {
                    ipItems.forEach(check => {
                        html += renderCheckRow(check, "tbl-row--indent2");
                    });
                }
            });
        } else {
            // Single IP hosts: checks align at 35px (indent1)
            items.sort((a, b) => a.check_type.localeCompare(b.check_type)).forEach(check => {
                html += renderCheckRow(check, "tbl-row--indent1");
            });
        }
    }
    return html;
}

function renderCheckRow(check, indentClass) {
    let cStatus = check.status ? "online" : "offline";
    let cLatVal = getLat(check);
    let cLat = cLatVal > 0 ? cLatVal.toFixed(2) + 'ms' : '--';
    const isSmartVerified = (check.message || "").includes("Verified via");

    return `
        <div class="tbl-row--service ${indentClass} tbl-row">
            <div class="tbl-col tbl-col--name">
                <div class="tree-indent-svc">
                    <i data-lucide="${getIcon(check.check_type || "")}" size="10" style="color: hsl(var(--foreground-subtle))"></i>
                    <span class="node-name-text" style="color: hsl(var(--foreground-muted));">${check.check_type}</span>
                </div>
            </div>
            <div class="tbl-col tbl-col--status"><span class="status-badge ${cStatus}"><span class="status-dot ${cStatus}"></span>${cStatus.toUpperCase()}</span></div>
            <div class="tbl-col tbl-col--latency"><span class="latency-value">${cLat}</span></div>
            <div class="tbl-col tbl-col--detail">
                <span style="color: ${isSmartVerified ? 'hsl(var(--status-online-bright))' : 'hsl(var(--foreground-subtle))'}; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
                    ${check.message}
                </span>
            </div>
        </div>
    `;
}

function getIcon(type) {
    if (type.includes("PING")) return "activity";
    if (type.includes("TCP")) return "shield";
    if (type.includes("UDP")) return "zap";
    return "box";
}

function toggleNode(key) {
    if (expandedNodes.has(key)) expandedNodes.delete(key);
    else expandedNodes.add(key);
    render();
}

window.toggleNode = toggleNode;

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
        if (syncText) syncText.innerText = "ENGINE: FETCHING";
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
            const isSyncing = checks.some(c => (c.message || "").includes("Synchronizing"));
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

        const finalSyncText = syncingNodes > 0 ? `ENGINE: SYNCING (${syncingNodes})` : 'ENGINE: SYNCED';
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
            if (syncText) syncText.innerText = "ENGINE: OFFLINE";
            if (engineBadge) engineBadge.classList.remove('refreshing');
            if (timer) timer.style.display = 'inline-flex';
        }, 1000);
    }
}

fetchTelemetry(false);
lucide.createIcons();
