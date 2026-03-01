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

function getIcon(type) {
    if (type.includes("PING")) return "activity";
    if (type.includes("TCP")) return "shield";
    if (type.includes("UDP")) return "zap";
    if (type.includes("HTTP")) return "globe";
    return "box";
}
