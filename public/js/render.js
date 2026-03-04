function init() {
    expandedNodes.add('initialized');
    expandedNodes.add('section-operational');
    expandedNodes.add('section-degraded');
}

function updateSearch(q) {
    query = q.toLowerCase();
    render();
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

    onlineContainer.innerHTML = renderTree(upItems, 'up', new Set());
    offlineContainer.innerHTML = renderTree(downItems, 'down', new Set());

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

function renderTree(items, sectionId, intelNodesSet) {
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
                        <div class="tbl-col tbl-col--actions"></div>
                    </div>
                    <div class="tbl-body">
        ${groupKeys.map(gKey => {
            const [sName, pAddr] = gKey.split('||');
            return renderServerGroup(sName, pAddr, category.groups[gKey].items, catId, intelNodesSet);
        }).join('')}
                    </div>
                </div>
            </div>
        `;
    });

    return html;
}

function renderServerGroup(sName, parentAddr, items, catId, intelNodesSet) {
    const isSubnet = parentAddr.includes("/") && !parentAddr.endsWith("/32");
    const groupKey = `group:${catId}:${sName}:${parentAddr}`;
    const isExpanded = expandedNodes.has(groupKey);

    const clusterStatus = items.every(i => i.status);
    const clusterPings = items.map(i => getLat(i)).filter(l => l > 0);
    const avgLatency = clusterPings.length > 0 ? (clusterPings.reduce((a, c) => a + c, 0) / clusterPings.length).toFixed(1) + 'ms' : '--';
    const clusterLossItems = items.map(i => i.packet_loss).filter(l => l !== null && typeof l === 'number');
    const avgLossVal = clusterLossItems.length > 0 ? (clusterLossItems.reduce((a, c) => a + c, 0) / clusterLossItems.length) : null;

    const formatLoss = (val) => {
        if (val === 0) return "0%";
        const formatted = val.toFixed(val < 1 ? 2 : 1).replace(/\.?0+$/, "");
        return `${formatted}%`;
    };
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
                    ${avgLossVal !== null ? `<div class="loss-box ${avgLossVal === 0 ? 'stable' : ''}">${formatLoss(avgLossVal)}</div>` : ''}
                </div>
            </div>
            <div class="tbl-col tbl-col--detail">
                <div style="display: flex; justify-content: space-between; align-items: center; width: 100%;">
                    <span style="color: hsl(var(--foreground-subtle)); font-size: 10px; letter-spacing: 0.02em;">
                        ${isSubnet ? `${new Set(items.map(i => i.target_address)).size} ACTIVE NODES` : (isMasked ? "" : "SINGLE ENDPOINT")}
                    </span>
                </div>
            </div>
            <div class="tbl-col tbl-col--actions">
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
                        return `<div class="loss-box ${ipAvgLoss === 0 ? 'stable' : ''}" style="opacity: 0.7;">${formatLoss(ipAvgLoss)}</div>`;
                    })()}
                            </div>
                        </div>
                        <div class="tbl-col tbl-col--detail"></div>
                        <div class="tbl-col tbl-col--actions">
                        </div>
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
                    ${check.packet_loss !== null && typeof check.packet_loss === 'number' ? `<div class="loss-box ${check.packet_loss === 0 ? 'stable' : ''}">${check.packet_loss === 0 ? "0%" : `${check.packet_loss.toFixed(1)}%`}</div>` : ''}
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
            <div class="tbl-col tbl-col--actions">
            </div>
        </div>
    `;
}


function toggleNode(key) {
    if (expandedNodes.has(key)) expandedNodes.delete(key);
    else expandedNodes.add(key);
    render();
}

window.toggleNode = toggleNode;
window.toggleNode = toggleNode;
window.updateSearch = updateSearch;
window.init = init;
window.render = render;
window.telemetry = telemetry;
