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
    if (typeof check.latency_ms === 'number' && check.latency_ms >= 0) return check.latency_ms;
    const match = (check.message || "").match(/(?:\[|@ | - )(\d+\.?\d*)ms/);
    if (match) return parseFloat(match[1]);
    return 0;
}

function render() {
    const onlineContainer = document.getElementById('online-categories-container');
    const offlineContainer = document.getElementById('offline-categories-container');
    if (!onlineContainer || !offlineContainer) return;

    if (telemetry.length === 0) {
        const loading = '<div style="padding: 1.5rem; color: var(--foreground-subtle);">CALIBRATING INFRASTRUCTURE MESH...</div>';
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
        if (!categoryGroups[cat]) {
            categoryGroups[cat] = {
                order: d.category_order ?? 999,
                groups: {}
            };
        }

        const groupKey = `${d.server_name}||${d.parent_address || ""}`;
        if (!categoryGroups[cat].groups[groupKey]) {
            categoryGroups[cat].groups[groupKey] = {
                order: d.server_order ?? 999,
                items: []
            };
        }
        categoryGroups[cat].groups[groupKey].items.push(d);
    });

    const sortedCategories = Object.keys(categoryGroups).sort((a, b) =>
        categoryGroups[a].order - categoryGroups[b].order
    );
    let html = '';

    sortedCategories.forEach(catName => {
        const category = categoryGroups[catName];
        const groupKeys = Object.keys(category.groups).sort((a, b) =>
            category.groups[a].order - category.groups[b].order
        );
        const catId = `cat-${sectionId}-${catName.replace(/\s+/g, '-').toLowerCase()}`;
        const isCatExpanded = expandedNodes.has(catId) || !expandedNodes.has('initialized');

        html += `
            <div class="category-block" style="margin-bottom: 2px;">
                <div class="tbl-row category-header" onclick="toggleNode('${catId}')">
                    <div class="tbl-col tbl-col--name">
                        <div class="node-name-cell" style="font-weight: 700;">
                            <div class="expand-btn-wrapper">
                                <i data-lucide="chevron-right" size="12" class="chevron ${isCatExpanded ? 'open' : ''}"></i>
                            </div>
                            <span class="node-name-text">${catName.toUpperCase()}</span>
                        </div>
                    </div>
                </div>
                <div class="category-content" style="display: ${isCatExpanded ? 'block' : 'none'}">
                    <div class="tbl-head">
                        <div class="tbl-col tbl-col--name">TERMINAL RESOURCE</div>
                        <div class="tbl-col tbl-col--status">STATUS</div>
                        <div class="tbl-col tbl-col--latency">TELEMETRY</div>
                        <div class="tbl-col tbl-col--detail">DIAGNOSIS</div>
                    </div>
                    <div class="tbl-body">
                        ${groupKeys.map(gKey => {
            const [sName, pAddr] = gKey.split('||');
            return renderServerGroup(sName, pAddr, category.groups[gKey].items, catId);
        }).join('')}
                    </div>
                </div>
            </div>
        `;
    });

    return html;
}

function renderServerGroup(sName, parentAddr, items, catId) {
    const isSubnet = parentAddr.includes("/") && !parentAddr.endsWith("/32");

    const groupKey = `group:${catId}:${sName}:${parentAddr}`;
    const isExpanded = expandedNodes.has(groupKey);

    const clusterStatus = items.every(i => i.status);
    const clusterPings = items.map(i => getLat(i)).filter(l => l > 0);
    const avgLatency = clusterPings.length > 0 ? (clusterPings.reduce((a, c) => a + c, 0) / clusterPings.length).toFixed(1) + 'ms' : '--';

    const clusterLossItems = items.map(i => i.packet_loss).filter(l => l !== null && typeof l === 'number');
    const avgLossVal = clusterLossItems.length > 0 ? (clusterLossItems.reduce((a, c) => a + c, 0) / clusterLossItems.length) : null;
    const avgLossText = avgLossVal !== null ? avgLossVal.toFixed(1) + '%' : '--';

    let status = clusterStatus ? "online" : "degraded";

    const isMasked = parentAddr === 'HIDDEN' || (parentAddr && parentAddr.startsWith('MASKED-')) || (parentAddr && parentAddr.includes('***'));
    const displayName = isMasked
        ? `${sName} <span style="opacity: 0.5; font-size: 0.9em;">(HIDDEN)</span>`
        : `${sName} <span style="opacity: 0.5; font-size: 0.9em;">(${parentAddr})</span>`;

    let html = `
        <div class="tbl-row tbl-row--server" onclick="event.stopPropagation(); toggleNode('${groupKey}')">
            <div class="tbl-col tbl-col--name">
                <div class="node-name-cell">
                    <div class="expand-btn-wrapper">
                        <button class="expand-btn">
                            <i data-lucide="chevron-right" size="10" class="chevron ${isExpanded ? 'open' : ''}"></i>
                        </button>
                    </div>
                    <span class="node-name-text">${displayName}</span>
                </div>
            </div>
            <div class="tbl-col tbl-col--status">
                <span class="status-badge ${status}"><span class="status-dot ${status}"></span>${status === 'online' ? 'OPERATIONAL' : 'DEGRADED'}</span>
            </div>
            <div class="tbl-col tbl-col--latency">
                <div class="metric-suite">
                    <span class="latency-value">${avgLatency}</span>
                    ${avgLossVal !== null ? `
                        <div class="loss-box ${avgLossVal === 0 ? 'stable' : ''}">
                            ${avgLossVal.toFixed(1)}%
                        </div>
                    ` : ''}
                </div>
            </div>
            <div class="tbl-col tbl-col--detail">
                <span style="color: hsl(var(--foreground-subtle)); font-size: 10px; letter-spacing: 0.02em;">
                    ${isSubnet ? `${new Set(items.map(i => i.target_address)).size} ACTIVE NODES` : (isMasked ? "" : "SINGLE ENDPOINT")}
                </span>
            </div>
        </div>
    `;

    if (isExpanded) {
        if (isSubnet) {
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

                const ipPings = ipItems.map(i => getLat(i)).filter(l => l > 0);
                const ipAvgLat = ipPings.length > 0 ? (ipPings.reduce((a, c) => a + c, 0) / ipPings.length).toFixed(1) + 'ms' : '--';

                html += `
                    <div class="tbl-row tbl-row--ip-group tbl-row--indent1" onclick="event.stopPropagation(); toggleNode('${ipKey}')">
                        <div class="tbl-col tbl-col--name">
                            <div class="node-name-cell">
                                <div class="expand-btn-wrapper">
                                    <button class="expand-btn">
                                        <i data-lucide="chevron-right" size="10" class="chevron ${isIpExpanded ? 'open' : ''}"></i>
                                    </button>
                                </div>
                                <span class="node-name-text">${ip === 'HIDDEN' || ip.startsWith('MASKED-') || ip.includes('***') ? '' : ip}</span>
                            </div>
                        </div>
                        <div class="tbl-col tbl-col--status">
                            <span class="status-badge ${status}"><span class="status-dot ${status}"></span>${status === 'online' ? 'OPERATIONAL' : 'DEGRADED'}</span>
                        </div>
                        <div class="tbl-col tbl-col--latency">
                            <div class="metric-suite">
                                <span class="latency-value" style="opacity: 0.8;">${ipAvgLat}</span>
                                ${(() => {
                        const ipLosses = ipItems.map(i => i.packet_loss).filter(l => l !== null && typeof l === 'number');
                        const ipAvgLoss = ipLosses.length > 0 ? (ipLosses.reduce((a, c) => a + c, 0) / ipLosses.length) : 0;
                        return `
                                        <div class="loss-box ${ipAvgLoss === 0 ? 'stable' : ''}" style="opacity: 0.7;">
                                            ${ipAvgLoss.toFixed(1)}%
                                        </div>
                                    `;
                    })()}
                            </div>
                        </div>
                        <div class="tbl-col tbl-col--detail"></div>
                    </div>
                `;

                if (isIpExpanded) {
                    ipItems.sort((a, b) => (a.check_order ?? 0) - (b.check_order ?? 0)).forEach(check => {
                        html += renderCheckRow(check, "tbl-row--indent2");
                    });
                }
            });
        } else {
            items.sort((a, b) => (a.check_order ?? 0) - (b.check_order ?? 0)).forEach(check => {
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
    const isSmartVerified = (check.message || "").toLowerCase().includes("verified via");
    const isFault = (check.message || "").toLowerCase().includes("fault") || (check.message || "").toLowerCase().includes("loss");

    const provider = check.provider_node ? check.provider_node.slice(0, 4).toUpperCase() : "---";

    return `
        <div class="tbl-row--service ${indentClass} tbl-row">
            <div class="tbl-col tbl-col--name">
                <div class="node-name-cell">
                    <div class="expand-btn-wrapper" style="border: none; background: transparent;">
                        <i data-lucide="${getIcon(check.check_type || "")}" size="10" style="color: hsl(var(--foreground-subtle))"></i>
                    </div>
                    <span class="node-name-text" style="color: hsl(var(--foreground-muted)); font-size: 11px;">${check.check_type}</span>
                </div>
            </div>
            <div class="tbl-col tbl-col--status"><span class="status-badge ${cStatus}"><span class="status-dot ${cStatus}"></span>${cStatus === 'online' ? 'ACTIVE' : 'OFFLINE'}</span></div>
            <div class="tbl-col tbl-col--latency">
                <div class="metric-suite">
                    <span class="latency-value">${cLat}</span>
                    ${check.packet_loss !== null && typeof check.packet_loss === 'number' ? `
                        <div class="loss-box ${check.packet_loss === 0 ? 'stable' : ''}">
                            ${check.packet_loss.toFixed(1)}%
                        </div>
                    ` : ''}
                </div>
            </div>
            <div class="tbl-col tbl-col--detail">
                <div style="display: flex; align-items: center; gap: 8px;">
                    <span style="font-family: 'JetBrains Mono', monospace; font-size: 8px; color: hsl(var(--foreground-subtle)); background: rgba(255,255,255,0.05); padding: 1px 4px; border-radius: 2px; border: 1px solid rgba(255,255,255,0.1);">[${provider}]</span>
                    <span style="color: ${isSmartVerified ? 'hsl(var(--status-online-bright))' : isFault ? 'hsl(var(--status-degraded-bright))' : 'hsl(var(--foreground-subtle))'}; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 11px;">
                        ${check.message.toUpperCase()}
                    </span>
                </div>
            </div>
        </div>
    `;
}

function getIcon(type) {
    if (type.includes("PING")) return "activity";
    if (type.includes("TCP")) return "shield";
    if (type.includes("UDP")) return "zap";
    if (type.includes("HTTP")) return "globe";
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

fetchTelemetry(false);
lucide.createIcons();
