# SPECTRA - Infrastructure Intelligence

A high-performance, asynchronous "Turbo Mesh" monitoring system built with Rust. SPECTRA provides enterprise-grade infrastructure visibility with sub-second telemetry, intelligent protocol fallback, and multi-channel alerting (ntfy.sh & Discord).

## Features

- **Turbo Mesh Engine**: High-concurrency monitoring (1500+ workers) with semaphore-based rate limiting.
- **Intelligent Handshake Fallback**: Automatically verifies host status via TCP service checks (SSH, HTTP, etc.) if ICMP pings are filtered by firewalls.
- **Mobile Push Notifications**: Native phone alerts via **ntfy.sh** (No account required, high-priority "Urgent" bypass).
- **Discord Integration**: Rich media embeds for status transitions with color-coded telemetry.
- **Micro-Telemetry**: Sub-millisecond latency tracking and multi-probe packet loss calculation.
- **Premium HUD**: High-fidelity web dashboard with "Infrastructure Intelligence" aesthetic, real-time clock, and smart status badges.
- **DNS Intelligence**: Low-latency resolution via Cloudflare 1.1.1.1 integration.

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
| `webhook_url` | string | Discord or generic webhook URL |
| `ntfy_topic` | string | Unique topic for mobile push alerts |
| `api_port` | u16 | Dashboard listener port |

## Notifications

### ntfy.sh (Mobile Push Setup)
SPECTRA uses **ntfy.sh** for instant, account-free mobile notifications.

1. **Get the App**: Install **ntfy** from the [App Store](https://apps.apple.com/app/ntfy/id1625396347) or [Google Play](https://play.google.com/store/apps/details?id=io.heckel.ntfy).
2. **Choose a Topic**: Pick a long, unique secret string (e.g., `spectra_alerts_7722_lucas`).
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
