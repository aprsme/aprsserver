# aprsserver-rust

## Live-Reloading Development

For a live development experience (auto-recompile and browser auto-refresh):

1. **Install cargo-watch** (if you haven't already):
   ```sh
   cargo install cargo-watch
   ```

2. **Run the server with auto-reload:**
   ```sh
   cargo watch -x run
   ```

3. **Open the web UI:**
   - Visit [http://localhost:14501/](http://localhost:14501/) in your browser.
   - The dashboard will auto-refresh when you change and recompile Rust code.

---

- The web UI polls the server for changes and reloads automatically.
- You do not need to manually refresh the browser or restart the server during development.

## S2S Peering (Server-to-Server)

To enable S2S peering, add a section like this to your `aprsserver.toml`:

```toml
# Port for incoming S2S connections (optional, default: 14579)
s2s_port = 14579

[[s2s_peers]]
host = "peer1.aprs.net"
port = 14580
passcode = 12345
peer_name = "peer1"

[[s2s_peers]]
host = "peer2.aprs.net"
port = 14580
passcode = 23456
# peer_name is optional
```

Each entry defines a peer to connect to as a server-to-server peer. 