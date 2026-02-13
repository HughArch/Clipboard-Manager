use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{timeout, Duration};
use tokio::net::{TcpListener, TcpStream};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{broadcast, mpsc, Mutex};
use uuid::Uuid;
use sha2::{Digest, Sha256};

const DEDUP_CAPACITY: usize = 512;
const FRAME_MAX_SIZE: usize = 6 * 1024 * 1024; // 6MB safety cap (images are limited to 5MB)

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanQueueRole {
    Off,
    Host,
    Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanQueueStatus {
    pub role: LanQueueRole,
    pub connected: bool,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub self_id: String,
    pub self_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanQueueMember {
    pub id: String,
    pub name: Option<String>,
    pub addr: Option<String>,
    pub is_self: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanClipboardItem {
    pub id: String,
    pub kind: String,
    pub payload: String,
    pub timestamp: String,
    pub origin: String,
    pub sender_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum LanQueueEnvelope {
    AuthRequest {
        password: String,
        client_id: String,
        client_name: Option<String>,
    },
    AuthResponse {
        ok: bool,
        reason: Option<String>,
    },
    ClipboardItem {
        item: LanClipboardItem,
    },
    MemberUpdate {
        members: Vec<LanQueueMember>,
    },
}

#[derive(Debug)]
struct DedupCache {
    order: VecDeque<String>,
    set: HashSet<String>,
    capacity: usize,
}

impl DedupCache {
    fn new(capacity: usize) -> Self {
        Self {
            order: VecDeque::new(),
            set: HashSet::new(),
            capacity,
        }
    }

    fn contains(&self, id: &str) -> bool {
        self.set.contains(id)
    }

    fn insert(&mut self, id: String) {
        if self.set.contains(&id) {
            return;
        }
        self.order.push_back(id.clone());
        self.set.insert(id);
        while self.order.len() > self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
    }
}

#[derive(Debug)]
struct PeerHandle {
    sender: mpsc::UnboundedSender<Vec<u8>>,
    name: Option<String>,
    addr: Option<String>,
}

#[derive(Debug)]
pub struct LanQueueState {
    role: LanQueueRole,
    host: Option<String>,
    port: Option<u16>,
    self_id: String,
    self_name: Option<String>,
    password_hash: Option<String>,
    host_listener: Option<tokio::task::JoinHandle<()>>,
    host_shutdown: Option<broadcast::Sender<()>>,
    client_task: Option<tokio::task::JoinHandle<()>>,
    client_write_task: Option<tokio::task::JoinHandle<()>>,
    client_sender: Option<mpsc::UnboundedSender<Vec<u8>>>,
    peers: HashMap<String, PeerHandle>,
    dedup: DedupCache,
}

impl Default for LanQueueState {
    fn default() -> Self {
        Self {
            role: LanQueueRole::Off,
            host: None,
            port: None,
            self_id: Uuid::new_v4().to_string(),
            self_name: None,
            password_hash: None,
            host_listener: None,
            host_shutdown: None,
            client_task: None,
            client_write_task: None,
            client_sender: None,
            peers: HashMap::new(),
            dedup: DedupCache::new(DEDUP_CAPACITY),
        }
    }
}

fn hash_password(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    let digest = hasher.finalize();
    hex::encode(digest)
}

fn normalize_name(name: Option<String>) -> Option<String> {
    name.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn current_status(state: &LanQueueState) -> LanQueueStatus {
    LanQueueStatus {
        role: state.role.clone(),
        connected: match state.role {
            LanQueueRole::Host => true,
            LanQueueRole::Client => state.client_sender.is_some(),
            LanQueueRole::Off => false,
        },
        host: state.host.clone(),
        port: state.port,
        self_id: state.self_id.clone(),
        self_name: state.self_name.clone(),
    }
}

fn make_members(state: &LanQueueState) -> Vec<LanQueueMember> {
    let mut members = Vec::new();
    members.push(LanQueueMember {
        id: state.self_id.clone(),
        name: state.self_name.clone(),
        addr: None,
        is_self: true,
    });
    for (id, peer) in &state.peers {
        members.push(LanQueueMember {
            id: id.clone(),
            name: peer.name.clone(),
            addr: peer.addr.clone(),
            is_self: false,
        });
    }
    members
}

async fn emit_members(app: &AppHandle, state: &LanQueueState) {
    let members = make_members(state);
    let _ = app.emit("lan-queue-members", members);
}

async fn broadcast_members_to_peers(state: &mut LanQueueState) {
    let members = make_members(state);
    let envelope = LanQueueEnvelope::MemberUpdate { members };
    if let Ok(payload) = serde_json::to_vec(&envelope) {
        let frame = build_frame(&payload);
        for peer in state.peers.values() {
            let _ = peer.sender.send(frame.clone());
        }
    }
}

fn build_frame(payload: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(4 + payload.len());
    let len = payload.len() as u32;
    buf.extend_from_slice(&len.to_be_bytes());
    buf.extend_from_slice(payload);
    buf
}

async fn read_frame<R>(stream: &mut R) -> Result<Vec<u8>, String>
where
    R: AsyncReadExt + Unpin,
{
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await.map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > FRAME_MAX_SIZE {
        return Err("Invalid frame size".to_string());
    }
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).await.map_err(|e| e.to_string())?;
    Ok(payload)
}

async fn write_frames(mut stream: OwnedWriteHalf, mut rx: mpsc::UnboundedReceiver<Vec<u8>>) {
    while let Some(frame) = rx.recv().await {
        if stream.write_all(&frame).await.is_err() {
            break;
        }
    }
}

async fn handle_host_connection(
    app: AppHandle,
    state: Arc<Mutex<LanQueueState>>,
    mut stream: TcpStream,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    let peer_addr = stream.peer_addr().ok().map(|addr| addr.to_string());
    let auth_payload = match read_frame(&mut stream).await {
        Ok(payload) => payload,
        Err(_) => return,
    };
    let envelope: LanQueueEnvelope = match serde_json::from_slice(&auth_payload) {
        Ok(value) => value,
        Err(_) => return,
    };

    let (client_id, client_name, password_ok) = match envelope {
        LanQueueEnvelope::AuthRequest {
            password,
            client_id,
            client_name,
        } => {
            let hash = hash_password(&password);
            let state_guard = state.lock().await;
            let ok = state_guard.password_hash.as_deref() == Some(hash.as_str());
            (client_id, normalize_name(client_name), ok)
        }
        _ => return,
    };

    let response = LanQueueEnvelope::AuthResponse {
        ok: password_ok,
        reason: if password_ok { None } else { Some("Invalid password".to_string()) },
    };
    if let Ok(response_payload) = serde_json::to_vec(&response) {
        let frame = build_frame(&response_payload);
        if stream.write_all(&frame).await.is_err() {
            return;
        }
    }

    if !password_ok {
        return;
    }

    let (read_half, write_half) = stream.into_split();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(write_frames(write_half, rx));

    {
        let mut state_guard = state.lock().await;
        state_guard.peers.insert(
            client_id.clone(),
            PeerHandle {
                sender: tx,
                name: client_name.clone(),
                addr: peer_addr.clone(),
            },
        );
        broadcast_members_to_peers(&mut state_guard).await;
        emit_members(&app, &state_guard).await;
    }

    let mut read_half = read_half;
    loop {
        let payload = tokio::select! {
            result = read_frame(&mut read_half) => {
                match result {
                    Ok(payload) => payload,
                    Err(_) => break,
                }
            }
            _ = shutdown_rx.recv() => {
                break;
            }
        };
        let envelope: LanQueueEnvelope = match serde_json::from_slice(&payload) {
            Ok(value) => value,
            Err(_) => continue,
        };
        match envelope {
            LanQueueEnvelope::ClipboardItem { item } => {
                let mut state_guard = state.lock().await;
                if state_guard.dedup.contains(&item.id) {
                    continue;
                }
                state_guard.dedup.insert(item.id.clone());
                let _ = app.emit("lan-clipboard-item", item.clone());
                for (peer_id, peer) in &state_guard.peers {
                    if peer_id == &client_id {
                        continue;
                    }
                    if let Ok(payload) = serde_json::to_vec(&LanQueueEnvelope::ClipboardItem { item: item.clone() }) {
                        let frame = build_frame(&payload);
                        let _ = peer.sender.send(frame);
                    }
                }
            }
            _ => {}
        }
    }

    {
        let mut state_guard = state.lock().await;
        state_guard.peers.remove(&client_id);
        broadcast_members_to_peers(&mut state_guard).await;
        emit_members(&app, &state_guard).await;
    }
}

async fn handle_client_stream(
    app: AppHandle,
    state: Arc<Mutex<LanQueueState>>,
    mut read_half: OwnedReadHalf,
) {
    loop {
        let payload = match read_frame(&mut read_half).await {
            Ok(payload) => payload,
            Err(_) => break,
        };
        let envelope: LanQueueEnvelope = match serde_json::from_slice(&payload) {
            Ok(value) => value,
            Err(_) => continue,
        };
        match envelope {
            LanQueueEnvelope::ClipboardItem { item } => {
                let mut state_guard = state.lock().await;
                if state_guard.dedup.contains(&item.id) {
                    continue;
                }
                state_guard.dedup.insert(item.id.clone());
                let _ = app.emit("lan-clipboard-item", item);
            }
            LanQueueEnvelope::MemberUpdate { members } => {
                let _ = app.emit("lan-queue-members", members);
            }
            _ => {}
        }
    }

    let mut state_guard = state.lock().await;
    state_guard.client_sender = None;
    state_guard.client_write_task = None;
    state_guard.role = LanQueueRole::Off;
    let _ = app.emit("lan-queue-status", current_status(&state_guard));
    let _ = app.emit("lan-queue-members", Vec::<LanQueueMember>::new());
}

#[tauri::command]
pub async fn lan_queue_start_host(
    app: AppHandle,
    port: u16,
    password: String,
    queue_name: Option<String>,
    member_name: Option<String>,
) -> Result<LanQueueStatus, String> {
    let state = app.state::<Arc<Mutex<LanQueueState>>>();
    let mut state_guard = state.inner().lock().await;

    if let Some(handle) = state_guard.host_listener.take() {
        handle.abort();
    }
    if let Some(shutdown) = state_guard.host_shutdown.take() {
        let _ = shutdown.send(());
    }
    if let Some(handle) = state_guard.client_task.take() {
        handle.abort();
    }
    if let Some(handle) = state_guard.client_write_task.take() {
        handle.abort();
    }
    state_guard.client_sender = None;
    state_guard.peers.clear();
    state_guard.role = LanQueueRole::Host;
    state_guard.host = Some("0.0.0.0".to_string());
    state_guard.port = Some(port);
    state_guard.self_name = normalize_name(member_name.clone().or(queue_name));
    state_guard.password_hash = Some(hash_password(&password));

    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|e| format!("Failed to bind host port: {}", e))?;

    let app_handle = app.clone();
    let state_arc = state.inner().clone();
    let (shutdown_tx, _) = broadcast::channel(1);
    state_guard.host_shutdown = Some(shutdown_tx.clone());
    let listener_handle = tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let app_handle = app_handle.clone();
                    let state_clone = Arc::clone(&state_arc);
                    let shutdown_rx = shutdown_tx.subscribe();
                    tokio::spawn(handle_host_connection(app_handle, state_clone, stream, shutdown_rx));
                }
                Err(_) => break,
            }
        }
    });

    state_guard.host_listener = Some(listener_handle);
    let status = current_status(&state_guard);
    let _ = app.emit("lan-queue-status", status.clone());
    emit_members(&app, &state_guard).await;
    Ok(status)
}

#[tauri::command]
pub async fn lan_queue_join(
    app: AppHandle,
    host: String,
    port: u16,
    password: String,
    member_name: Option<String>,
) -> Result<LanQueueStatus, String> {
    let state = app.state::<Arc<Mutex<LanQueueState>>>();
    let mut state_guard = state.inner().lock().await;

    if let Some(handle) = state_guard.host_listener.take() {
        handle.abort();
    }
    if let Some(shutdown) = state_guard.host_shutdown.take() {
        let _ = shutdown.send(());
    }
    if let Some(handle) = state_guard.client_task.take() {
        handle.abort();
    }
    if let Some(handle) = state_guard.client_write_task.take() {
        handle.abort();
    }
    state_guard.client_sender = None;
    state_guard.peers.clear();
    state_guard.role = LanQueueRole::Client;
    state_guard.host = Some(host.clone());
    state_guard.port = Some(port);
    state_guard.self_name = normalize_name(member_name);
    state_guard.password_hash = None;

    let mut stream = match timeout(Duration::from_secs(3), TcpStream::connect((host.as_str(), port))).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(e)) => return Err(format!("Failed to connect: {}", e)),
        Err(_) => return Err("Connection timeout (3s)".to_string()),
    };

    let auth = LanQueueEnvelope::AuthRequest {
        password,
        client_id: state_guard.self_id.clone(),
        client_name: state_guard.self_name.clone(),
    };
    let auth_payload = serde_json::to_vec(&auth).map_err(|e| e.to_string())?;
    timeout(Duration::from_secs(3), stream.write_all(&build_frame(&auth_payload)))
        .await
        .map_err(|_| "Connection timeout (3s)".to_string())?
        .map_err(|e| e.to_string())?;

    let response_payload = timeout(Duration::from_secs(3), read_frame(&mut stream))
        .await
        .map_err(|_| "Connection timeout (3s)".to_string())??;
    let response: LanQueueEnvelope = serde_json::from_slice(&response_payload).map_err(|e| e.to_string())?;
    match response {
        LanQueueEnvelope::AuthResponse { ok, reason } => {
            if !ok {
                return Err(reason.unwrap_or_else(|| "Authentication failed".to_string()));
            }
        }
        _ => return Err("Invalid auth response".to_string()),
    }

    let (read_half, write_half) = stream.into_split();
    let (tx, rx) = mpsc::unbounded_channel();
    let write_handle = tokio::spawn(write_frames(write_half, rx));
    state_guard.client_sender = Some(tx);
    state_guard.client_write_task = Some(write_handle);

    let app_handle = app.clone();
    let state_arc = state.inner().clone();
    let client_handle = tokio::spawn(handle_client_stream(app_handle, Arc::clone(&state_arc), read_half));
    state_guard.client_task = Some(client_handle);

    let status = current_status(&state_guard);
    let _ = app.emit("lan-queue-status", status.clone());
    Ok(status)
}

#[tauri::command]
pub async fn lan_queue_leave(app: AppHandle) -> Result<(), String> {
    let state = app.state::<Arc<Mutex<LanQueueState>>>();
    let mut state_guard = state.inner().lock().await;

    if let Some(handle) = state_guard.host_listener.take() {
        handle.abort();
    }
    if let Some(shutdown) = state_guard.host_shutdown.take() {
        let _ = shutdown.send(());
    }
    if let Some(handle) = state_guard.client_task.take() {
        handle.abort();
    }
    if let Some(handle) = state_guard.client_write_task.take() {
        handle.abort();
    }
    state_guard.client_sender = None;
    state_guard.peers.clear();
    state_guard.role = LanQueueRole::Off;
    state_guard.host = None;
    state_guard.port = None;
    state_guard.password_hash = None;

    let status = current_status(&state_guard);
    let _ = app.emit("lan-queue-status", status);
    let _ = app.emit("lan-queue-members", Vec::<LanQueueMember>::new());
    Ok(())
}

#[tauri::command]
pub async fn lan_queue_send(
    app: AppHandle,
    mut item: LanClipboardItem,
) -> Result<(), String> {
    let state = app.state::<Arc<Mutex<LanQueueState>>>();
    let mut state_guard = state.inner().lock().await;

    if item.id.trim().is_empty() {
        item.id = Uuid::new_v4().to_string();
    }
    if item.origin.trim().is_empty() {
        item.origin = state_guard.self_id.clone();
    }
    if item.sender_name.is_none() {
        item.sender_name = state_guard.self_name.clone();
    }

    if state_guard.dedup.contains(&item.id) {
        return Ok(());
    }
    state_guard.dedup.insert(item.id.clone());

    let envelope = LanQueueEnvelope::ClipboardItem { item };
    let payload = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
    let frame = build_frame(&payload);

    match state_guard.role {
        LanQueueRole::Host => {
            for peer in state_guard.peers.values() {
                let _ = peer.sender.send(frame.clone());
            }
        }
        LanQueueRole::Client => {
            if let Some(sender) = &state_guard.client_sender {
                let _ = sender.send(frame);
            }
        }
        LanQueueRole::Off => {}
    }

    let status = current_status(&state_guard);
    let _ = app.emit("lan-queue-status", status);
    Ok(())
}

#[tauri::command]
pub async fn lan_queue_status(app: AppHandle) -> Result<LanQueueStatus, String> {
    let state = app.state::<Arc<Mutex<LanQueueState>>>();
    let state_guard = state.inner().lock().await;
    Ok(current_status(&state_guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let payload = b"{\"hello\":\"world\"}".to_vec();
        let frame = build_frame(&payload);
        assert!(frame.len() >= 4);
        let len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        assert_eq!(len, payload.len());
        let decoded = frame[4..].to_vec();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn dedup_cache_evicts_oldest() {
        let mut cache = DedupCache::new(3);
        cache.insert("a".to_string());
        cache.insert("b".to_string());
        cache.insert("c".to_string());
        assert!(cache.contains("a"));
        cache.insert("d".to_string());
        assert!(!cache.contains("a"));
        assert!(cache.contains("b"));
        assert!(cache.contains("c"));
        assert!(cache.contains("d"));
    }
}
