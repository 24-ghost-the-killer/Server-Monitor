# SPECTRA - Infrastructure Intelligence

A high-performance, asynchronous "Turbo Mesh" monitoring system built with Rust. SPECTRA provides enterprise-grade infrastructure visibility with sub-second telemetry, intelligent protocol fallback, and multi-channel alerting (ntfy.sh & Discord).

## Features

- **Turbo Mesh V4 Engine**: Multi-node high-concurrency monitoring (1500+ workers) with semaphore-based rate limiting.
- **Micro-Second Precision Pulse**: Aggressive 1-second heartbeat frequency for near-instant node failover.
- **Fair Workload Distribution**: Deterministic round-robin allocation ensuring perfectly even task distribution among cluster members.
- **Intelligent Handshake Fallback**: Verifies host status via TCP service checks (SSH, HTTP, RDP, etc.) if ICMP pings are filtered.
- **Mobile Push Notifications**: Urgent phone alerts via **ntfy.sh** with priority bypass and custom tags.
- **Discord Integration**: Rich media embeds with color-coded mesh transition telemetry.
- **Cloudflare DNS Integration**: Optimized 1.1.1.1 resolution for high-accuracy global monitoring.
- **Advanced UI HUD**: Glassmorphism dashboard with real-time stats, tree-view hierarchy, and search filtering.

## Live Preview

A public instance of the dashboard is available for preview:

[http://5.175.246.32:3000/](http://5.175.246.32:3000/)

## Getting Started

### Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd "Server Monitor"
```

2. Build and Run:
```bash
cargo run --release
```

### Configuration

Create a `config.json` file in the project root:

```json
{
    "check_interval": 20,
    "max_concurrency": 99,
    "api_port": 3000,
    "webhook_url": "https://discord.com/api/webhooks/...",
    "ntfy_topic": "your_secret_topic_name",
    "redis_url": "redis://<redis-ip>:6379",
    "redis_prefix": "spectra",
    "max_checks_per_second": 50,
    "enable_dashboard": null,
    "categories": [
        {
            "name": "Cloud Infrastructure",
            "servers": [
                {
                    "name": "Primary Hypervisor",
                    "address": "1.2.3.4",
                    "checks": [
                        { "type": "Ping", "count": 4, "timeout_ms": 1000 },
                        { "type": "TcpPort", "port": 22, "count": 2 }
                    ]
                }
            ]
        }
    ]
}
```

### Configuration Options

| Option | Type | Description |
|--------|------|-------------|
| `check_interval` | u64 | Seconds between monitoring cycles |
| `max_concurrency` | usize | Maximum simultaneous check workers |
| `max_checks_per_second` | u64 | Global cluster rate limiting for outgoing checks |
| `hide_endpoints` | bool | Enable cryptographic masking of IP addresses in the API/UI |
| `webhook_url` | string | Discord or generic webhook URL |
| `ntfy_topic` | string | Unique topic for mobile push alerts |
| `api_port` | u16 | Dashboard listener port |
| `redis_url` | string | Redis connection string (e.g., `redis://<redis-ip>:6379`) |
| `redis_prefix` | string | Prefix for Redis keys to isolate multiple clusters |
| `enable_dashboard` | bool/null | Explicitly enable/disable the web UI (defaults to `false` if Redis is connected, `true` if Standalone) |

## Redis Clustering & Caching

SPECTRA supports distributed monitoring across multiple nodes using Redis. When `redis_url` is configured:

1.  **Workload Distribution**: Nodes use a **Fair Round-Robin** algorithm to perfectly balance monitoring tasks. For example, in a two-node cluster, the workload is distributed with a 50/50 split. 
2.  **Fast Failover**: Node heartbeats are refreshed every 1 second. If a node disconnects, the mesh detects the "timeout" within 5 seconds and redistributes its workload across the remaining healthy nodes.
3.  **Global Mesh State**: All nodes synchronize results to a high-speed Redis hash. The dashboard (if enabled on a node) provides a unified, cluster-wide view of all monitored resources.
4.  **Distributed Locking**: A built-in lock mechanism prevents multiple nodes from checking the same endpoint simultaneously during mesh transitions.
5.  **Config Pruning**: Redundant results from deleted config servers are automatically purged from the Redis mesh state upon startup.

### Clustering Configuration Example

```json
{
    "check_interval": 30,
    "redis_url": "redis://localhost:6379",
    "redis_prefix": "spectra-prod",
    "categories": [...]
}
```

## Notifications

### ntfy.sh (Mobile Push Setup)
SPECTRA uses **ntfy.sh** for instant, account-free mobile notifications.

1. **Get the App**: Install **ntfy** from the [App Store](https://apps.apple.com/app/ntfy/id1625396347) or [Google Play](https://play.google.com/store/apps/details?id=io.heckel.ntfy).
2. **Choose a Topic**: Pick a long, unique secret string (e.g., `spectra_alerts_7722`).
3. **Configure SPECTRA**: Update your `config.json` with this topic:
   ```json
   "ntfy_topic": "your_secret_topic"
   ```
4. **Subscribe on Phone**:
   - Open the ntfy app.
   - Tap **(+)** or "Subscribe to topic".
   - Enter your secret topic name.
5. **Receive Alerts**: SPECTRA will now send "Urgent" (Priority 5) alerts to your phone whenever a node goes down, and a standard alert when it recovers. No login or API keys required.

### Discord Setup
Simply provide your Discord Webhook URL in `config.json`. SPECTRA automatically detects the Discord domain and switches to rich Embed mode with status-colored sidebars.

## Web Dashboard

Access the real-time telemetry at: `http://localhost:3000`

The dashboard features:
- **MESH: SYNCED** Status Engine
- **TERMINAL RESOURCE** status tracking
- **TELEMETRY** HUD (Latency & Packet Loss)
- **DIAGNOSIS** intelligent feedback

## API Endpoints

### Get Telemetry Mesh
`GET /api/stats`

Returns aggregated telemetry data:
```json
[
    {
        "server_name": "A-Zone Server",
        "target_address": "5.175.246.32",
        "check_type": "TCP:22",
        "status": true,
        "latency_ms": 45.2,
        "packet_loss": 0.0,
        "message": "TCP Interface: Established"
    }
]
```

## Performance & Logistics

SPECTRA is built on **Tokio** and **Axum**, leveraging:
- **Semaphore Workers**: Prevents resource exhaustion on large clusters.
- **TCP Handshake Verification**: Better accuracy than pure ICMP.
- **Zero-Allocation Logging**: Uses `tracing` for high-throughput logging.

## License

This project is provided as-is for infrastructure monitoring purposes.
