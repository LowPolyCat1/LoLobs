# League Rank Overlay

A lightweight, real-time desktop overlay backend built in **Rust**. This tool hooks into the League Client (LCU) to broadcast your current rank, LP, and session win/loss stats to a web-based overlay via WebSockets. Perfect for OBS, XSplit, or browser-source based stream overlays.

---

## What it does

* **Real-time Rank Updates:** Automatically detects changes in your Tier, Division, and LP without needing to refresh.
* **Session Tracking:** Tracks your wins and losses for the current session (resets when the app is restarted).
* **Summoner Info:** Fetches your Riot ID (Name + Tagline) and profile data.
* **WebSocket Architecture:** Acts as a local server that "blasts" data to any connected client, allowing for ultra-low latency updates.

---

## How it works

The program operates in three main layers:

1. **LCU Listener:** Using the `irelia` crate, the app discovers the League of Legends client locally and subscribes to its internal WebSocket events. It specifically watches for updates on the `/lol-ranked/v1/current-ranked-stats` endpoint.
2. **Rust Backend (Actix):** When the LCU sends an update, the Rust backend processes the JSON, calculates the session-specific wins/losses, and broadcasts the result to all connected WebSocket clients.
3. **Web Frontend:** A JavaScript-based overlay (`overlay.js`) connects to the local Rust server. It parses the incoming JSON to update the HTML DOM, change rank icons (sourced from CommunityDragon), and trigger animations.

---

## Getting Started

### Prerequisites

* **League of Legends** must be running.
* **Rust** (if compiling from source).

### Installation

1. Clone the repository.
2. Build the project:

```bash
cargo build --release

```

1. Run the executable:

```bash
./target/release/rank-overlay

```

1. Add `overlay.html` as a **Browser Source** in OBS at `http://127.0.0.1:2009/ws` (or simply open the file in your browser).

---

## Documentation

For a detailed breakdown of the JSON payloads and how to customize the frontend, please refer to the:

**[WebSocket Message Documentation](./docs.md)**

---

## Disclaimer

**This project is not officially supported by Riot Games.**

* This tool is a third-party application using the LCU (League Client Update) API.
* While it only reads data and does not modify game files or provide a competitive advantage, use it at your own risk.
* Riot Games, League of Legends, and all associated properties are trademarks or registered trademarks of Riot Games, Inc.
