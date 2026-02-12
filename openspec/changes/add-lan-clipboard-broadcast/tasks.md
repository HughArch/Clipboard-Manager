## 1. Foundations

- [x] 1.1 Review existing clipboard capture and history write flow to identify hook points
- [x] 1.2 Extend settings model for LAN queue config (role, host, port, password)
- [x] 1.3 Add Rust module skeleton for LAN queue service and message types

## 2. Backend LAN Service

- [x] 2.1 Implement TCP host listener and connection manager for queue members
- [x] 2.2 Implement client join flow with password validation and error codes
- [x] 2.3 Implement message framing + JSON envelope for text/image payloads
- [x] 2.4 Implement broadcast fan-out to connected members
- [x] 2.5 Implement dedup cache and rebroadcast suppression

## 3. Clipboard Integration

- [x] 3.1 Emit broadcast when local user copies text or image while in a queue
- [x] 3.2 Handle inbound broadcast and insert into clipboard history with LAN source marker
- [x] 3.3 Wire sender identity into history or metadata when available

## 4. Frontend UI

- [x] 4.1 Add UI to create/join/leave queue and show host info
- [x] 4.2 Add member list/status panel and connection state indicators
- [x] 4.3 Add user-facing errors/toasts for join failures and network issues

## 5. Validation & Docs

- [x] 5.1 Add tests for protocol encode/decode and dedup behavior
- [ ] 5.2 Run manual test checklist for two clients (text + image broadcast)
- [x] 5.3 Document LAN queue feature and usage notes

