let telemetry = [];
let expandedNodes = new Set(['section-operational', 'section-degraded', 'initialized']);
let query = "";
const REFRESH_INTERVAL_SEC = 30;
let lastFetchSec = -1;
let isActivelyFetching = false;
